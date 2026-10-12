//! App-name matching by normalized fuzzy score (FR-CMD-05).

/// Lower-case alphanumerics with single spaces.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

fn bigrams(s: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    chars.windows(2).map(|w| (w[0], w[1])).collect()
}

/// Sørensen–Dice coefficient on character bigrams.
fn dice(a: &str, b: &str) -> f64 {
    let (x, mut y) = (bigrams(a), bigrams(b));
    if x.is_empty() || y.is_empty() {
        return if a.replace(' ', "") == b.replace(' ', "") {
            1.0
        } else {
            0.0
        };
    }
    let total = x.len() + y.len();
    let mut hits = 0;
    for g in &x {
        if let Some(i) = y.iter().position(|h| h == g) {
            hits += 1;
            y.swap_remove(i);
        }
    }
    2.0 * hits as f64 / total as f64
}

/// Every query token is, in order, a prefix of the next word or the initials of
/// the next few words, and all words are used: "vs code" for "visual studio code".
fn tokens_align(q: &str, words: &[&str]) -> bool {
    let mut wi = 0;
    for t in q.split(' ') {
        if wi >= words.len() {
            return false;
        }
        if words[wi].starts_with(t) {
            wi += 1;
            continue;
        }
        let n = t.chars().count();
        if n >= 2 && wi + n <= words.len() {
            let initials: String = words[wi..wi + n]
                .iter()
                .filter_map(|w| w.chars().next())
                .collect();
            if initials == t {
                wi += n;
                continue;
            }
        }
        return false;
    }
    wi == words.len()
}

/// Score in 0..=1 of how well `query` names `candidate`.
pub fn score(query: &str, candidate: &str) -> f64 {
    let q = normalize(query);
    let c = normalize(candidate);
    if q.is_empty() || c.is_empty() {
        return 0.0;
    }
    if q == c {
        return 1.0;
    }
    let qc = q.replace(' ', "");
    let cc = c.replace(' ', "");
    if qc == cc {
        return 0.98;
    }
    let words: Vec<&str> = c.split(' ').collect();
    if c.starts_with(&q) {
        // "spotify" vs "spotify premium": strong, scaled by coverage.
        return 0.9 + 0.08 * (q.len() as f64 / c.len() as f64);
    }
    if words.iter().any(|w| *w == q) {
        return 0.85 + 0.05 * (q.len() as f64 / c.len() as f64);
    }
    // Acronym of all words: "vsc" for "visual studio code".
    let acronym: String = words.iter().filter_map(|w| w.chars().next()).collect();
    if acronym.len() >= 2 && acronym == qc {
        return 0.8;
    }
    if tokens_align(&q, &words) {
        return 0.82;
    }
    if c.contains(&q) {
        return 0.75;
    }
    0.7 * dice(&q, &c)
}

pub const MIN_SCORE: f64 = 0.45;
pub const CLEAR_LEAD: f64 = 0.08;

#[derive(Debug, Clone, PartialEq)]
pub enum Resolution<T> {
    Launch(T),
    Choose(Vec<T>),
    NotFound,
}

/// Picks the best candidate if it is clearly ahead, else up to 3 options.
pub fn resolve<T: Clone>(query: &str, items: &[T], name: impl Fn(&T) -> &str) -> Resolution<T> {
    let mut scored: Vec<(f64, &T)> = items
        .iter()
        .map(|i| (score(query, name(i)), i))
        .filter(|(s, _)| *s >= MIN_SCORE)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    // Duplicate names (same app in both Start Menu folders) count once.
    let mut seen = std::collections::HashSet::new();
    scored.retain(|(_, i)| seen.insert(normalize(name(i))));
    match scored.as_slice() {
        [] => Resolution::NotFound,
        [(_, only)] => Resolution::Launch((*only).clone()),
        [(a, best), (b, _), ..] if a - b >= CLEAR_LEAD || *a >= 0.999 => {
            Resolution::Launch((*best).clone())
        }
        many => Resolution::Choose(many.iter().take(3).map(|(_, i)| (*i).clone()).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<&'static str> {
        vec![
            "Spotify",
            "Unity Hub",
            "Unity 2022.3.10f1",
            "Notepad",
            "Notepad++",
            "Visual Studio Code",
            "Visual Studio 2022",
            "Microsoft Edge",
            "Calculator",
            "Uninstall Spotify",
        ]
    }

    fn pick(q: &str) -> Resolution<&'static str> {
        resolve(q, &names(), |s| s)
    }

    #[test]
    fn exact_and_prefix() {
        assert_eq!(pick("spotify"), Resolution::Launch("Spotify"));
        assert_eq!(
            pick("Open Calculator".trim_start_matches("Open ")),
            Resolution::Launch("Calculator")
        );
        assert_eq!(pick("edge"), Resolution::Launch("Microsoft Edge"));
        assert_eq!(pick("notepad"), Resolution::Launch("Notepad"));
    }

    #[test]
    fn ambiguous_gives_choices() {
        match pick("unity") {
            Resolution::Choose(c) => {
                assert!(c.contains(&"Unity Hub"));
                assert!(c.len() <= 3);
            }
            Resolution::Launch(x) => assert!(x.starts_with("Unity")),
            other => panic!("unexpected {other:?}"),
        }
        match pick("visual studio") {
            Resolution::Choose(c) => assert_eq!(c.len(), 2),
            other => panic!("expected choice, got {other:?}"),
        }
    }

    #[test]
    fn fuzzy_and_missing() {
        assert_eq!(pick("vs code"), Resolution::Launch("Visual Studio Code"));
        assert_eq!(pick("spotfy"), Resolution::Launch("Spotify"));
        assert_eq!(pick("photoshop"), Resolution::NotFound);
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize("  Notepad++ (x64) "), "notepad x64");
    }
}
