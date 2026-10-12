//! Chord state machine (section 7.1). Pure logic, no Win32: the hook feeds it
//! key events and timer ticks and acts on its decisions. No allocation on the
//! hot path beyond the small output struct.

use std::time::{Duration, Instant};

use crate::input::keys::{
    VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE,
};

pub const ARM_DELAY: Duration = Duration::from_millis(120);
pub const ESC_WINDOW: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChordAction {
    Dictation,
    Circle,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChordDef {
    pub action: ChordAction,
    pub mods: Mods,
    /// A non-modifier key that is part of the chord (Space in Ctrl+Alt+Space).
    pub trigger: Option<u16>,
}

impl ChordDef {
    /// Parses "ctrl+win", "ctrl+alt+space", ... (validated against the allowed list elsewhere).
    pub fn parse(action: ChordAction, spec: &str) -> Option<ChordDef> {
        let mut mods = Mods::default();
        let mut trigger = None;
        for p in spec.split('+') {
            match p.trim().to_ascii_lowercase().as_str() {
                "ctrl" => mods.ctrl = true,
                "shift" => mods.shift = true,
                "alt" => mods.alt = true,
                "win" => mods.win = true,
                "space" => trigger = Some(VK_SPACE),
                _ => return None,
            }
        }
        let count = [mods.ctrl, mods.shift, mods.alt, mods.win]
            .iter()
            .filter(|b| **b)
            .count();
        if count == 0 || (count < 2 && trigger.is_none()) {
            return None;
        }
        Some(ChordDef {
            action,
            mods,
            trigger,
        })
    }

    fn uses_key(&self, vk: u16) -> bool {
        match vk {
            VK_LCONTROL | VK_RCONTROL | VK_CONTROL => self.mods.ctrl,
            VK_LSHIFT | VK_RSHIFT | VK_SHIFT => self.mods.shift,
            VK_LMENU | VK_RMENU | VK_MENU => self.mods.alt,
            VK_LWIN | VK_RWIN => self.mods.win,
            other => self.trigger == Some(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    ChordDown(ChordAction),
    ChordUp(ChordAction),
    EmergencyStop,
    /// A physical key press while takeover detection is on (FR-CU-14).
    PhysicalInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Arming { idx: usize, since: Instant },
    Active { idx: usize },
}

/// What the hook must do with the current event.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Do not pass the event on.
    pub swallow: bool,
    /// Swallow this Win release and inject mask key + Win up instead (FR-HK-03).
    pub mask_win: bool,
    /// Start the activation timer.
    pub arm_timer: bool,
    pub events: [Option<HookEvent>; 2],
}

impl Decision {
    fn push(&mut self, e: HookEvent) {
        if self.events[0].is_none() {
            self.events[0] = Some(e);
        } else {
            self.events[1] = Some(e);
        }
    }
}

pub struct ChordMachine {
    chords: Vec<ChordDef>,
    held: [bool; 256],
    phase: Phase,
    /// A non-modifier key was pressed while modifiers were held; no arming until
    /// all modifiers are released (FR-HK-02).
    tainted: bool,
    /// A Win chord armed; the next Win release needs the mask key.
    win_dirty: bool,
    trigger_swallowed: bool,
    esc_times: [Option<Instant>; 3],
    pub paused: bool,
}

fn is_mod(vk: u16) -> bool {
    matches!(
        vk,
        VK_LCONTROL
            | VK_RCONTROL
            | VK_CONTROL
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_SHIFT
            | VK_LMENU
            | VK_RMENU
            | VK_MENU
            | VK_LWIN
            | VK_RWIN
    )
}

impl ChordMachine {
    pub fn new(chords: Vec<ChordDef>) -> Self {
        Self {
            chords,
            held: [false; 256],
            phase: Phase::Idle,
            tainted: false,
            win_dirty: false,
            trigger_swallowed: false,
            esc_times: [None; 3],
            paused: false,
        }
    }

    pub fn set_chords(&mut self, chords: Vec<ChordDef>) {
        self.chords = chords;
        self.phase = Phase::Idle;
    }

    pub fn is_active(&self) -> bool {
        matches!(self.phase, Phase::Active { .. })
    }

    fn mods(&self) -> Mods {
        let h = |a: u16, b: u16| self.held[a as usize] || self.held[b as usize];
        Mods {
            ctrl: h(VK_LCONTROL, VK_RCONTROL) || self.held[VK_CONTROL as usize],
            shift: h(VK_LSHIFT, VK_RSHIFT) || self.held[VK_SHIFT as usize],
            alt: h(VK_LMENU, VK_RMENU) || self.held[VK_MENU as usize],
            win: h(VK_LWIN, VK_RWIN),
        }
    }

    fn any_mod_held(&self) -> bool {
        let m = self.mods();
        m.ctrl || m.shift || m.alt || m.win
    }

    fn matching_chord(&self) -> Option<usize> {
        let m = self.mods();
        self.chords.iter().position(|c| {
            c.mods == m && c.trigger.map(|t| self.held[t as usize]).unwrap_or(true) && {
                // No non-modifier keys other than the trigger may be down.
                (0..256u16).all(|vk| !self.held[vk as usize] || is_mod(vk) || Some(vk) == c.trigger)
            }
        })
    }

    fn try_arm(&mut self, now: Instant, d: &mut Decision) {
        if self.paused || self.tainted {
            self.phase = Phase::Idle;
            return;
        }
        match self.matching_chord() {
            Some(idx) => {
                self.phase = Phase::Arming { idx, since: now };
                if self.chords[idx].mods.win {
                    self.win_dirty = true;
                }
                d.arm_timer = true;
            }
            None => self.phase = Phase::Idle,
        }
    }

    /// Feeds a key event that did not come from Wisp itself.
    /// `physical` is false for events injected by other software.
    pub fn on_key(
        &mut self,
        vk: u16,
        down: bool,
        physical: bool,
        report_physical: bool,
        now: Instant,
    ) -> Decision {
        let mut d = Decision::default();
        let vk = vk & 0xFF;
        let repeat = down && self.held[vk as usize];

        if down && !repeat && vk == VK_ESCAPE {
            self.esc_times.rotate_left(1);
            self.esc_times[2] = Some(now);
            if let Some(first) = self.esc_times[0] {
                if now.duration_since(first) <= ESC_WINDOW {
                    d.push(HookEvent::EmergencyStop);
                    self.esc_times = [None; 3];
                }
            }
        }

        if down {
            self.held[vk as usize] = true;
            let part_of_current = match self.phase {
                Phase::Arming { idx, .. } | Phase::Active { idx } => self.chords[idx].uses_key(vk),
                Phase::Idle => false,
            };
            if report_physical && physical && !repeat && vk != VK_ESCAPE && !part_of_current {
                d.push(HookEvent::PhysicalInput);
            }
            if repeat {
                if part_of_current && self.trigger_swallowed && !is_mod(vk) {
                    d.swallow = true;
                }
                return d;
            }
            let is_trigger = self.chords.iter().any(|c| c.trigger == Some(vk));
            if is_mod(vk) || is_trigger {
                match self.phase {
                    Phase::Idle | Phase::Arming { .. } => {
                        if !is_mod(vk) && self.tainted {
                            // A trigger key after a stray key: treat as a normal key.
                        } else {
                            self.try_arm(now, &mut d);
                            if let Phase::Arming { idx, .. } = self.phase {
                                if self.chords[idx].trigger == Some(vk) {
                                    d.swallow = true;
                                    self.trigger_swallowed = true;
                                }
                            }
                        }
                    }
                    Phase::Active { .. } => {}
                }
                if !is_mod(vk) && !d.swallow && self.any_mod_held() {
                    self.tainted = true;
                    if matches!(self.phase, Phase::Arming { .. }) {
                        self.phase = Phase::Idle;
                    }
                }
            } else {
                // A non-modifier key: abandon arming and pass everything through.
                if let Phase::Arming { .. } = self.phase {
                    self.phase = Phase::Idle;
                }
                if self.any_mod_held() {
                    self.tainted = true;
                }
            }
            return d;
        }

        // Key up.
        let was_held = self.held[vk as usize];
        self.held[vk as usize] = false;
        match self.phase {
            Phase::Arming { idx, .. } | Phase::Active { idx } if self.chords[idx].uses_key(vk) => {
                if matches!(self.phase, Phase::Active { .. }) {
                    d.push(HookEvent::ChordUp(self.chords[idx].action));
                }
                self.phase = Phase::Idle;
            }
            _ => {}
        }
        if self.trigger_swallowed && self.chords.iter().any(|c| c.trigger == Some(vk)) {
            d.swallow = true;
            self.trigger_swallowed = false;
        }
        if (vk == VK_LWIN || vk == VK_RWIN) && self.win_dirty && was_held {
            d.mask_win = true;
            self.win_dirty = false;
        }
        if !self.any_mod_held() {
            self.tainted = false;
            self.win_dirty = self.win_dirty && self.mods().win;
        }
        d
    }

    /// Timer tick: Arming becomes Active after the delay (FR-HK-01).
    /// Returns the remaining delay if still arming.
    pub fn tick(&mut self, now: Instant) -> (Option<HookEvent>, Option<Duration>) {
        if let Phase::Arming { idx, since } = self.phase {
            let elapsed = now.duration_since(since);
            if elapsed >= ARM_DELAY {
                self.phase = Phase::Active { idx };
                return (Some(HookEvent::ChordDown(self.chords[idx].action)), None);
            }
            return (None, Some(ARM_DELAY - elapsed));
        }
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> ChordMachine {
        ChordMachine::new(vec![
            ChordDef::parse(ChordAction::Dictation, "ctrl+win").unwrap(),
            ChordDef::parse(ChordAction::Circle, "ctrl+shift").unwrap(),
            ChordDef::parse(ChordAction::Command, "ctrl+alt+space").unwrap(),
        ])
    }

    const A: u16 = 0x41;
    const D: u16 = 0x44;

    struct Feed {
        m: ChordMachine,
        t: Instant,
        events: Vec<HookEvent>,
    }

    impl Feed {
        fn new() -> Self {
            Self {
                m: machine(),
                t: Instant::now(),
                events: vec![],
            }
        }
        fn key(&mut self, vk: u16, down: bool) -> Decision {
            let d = self.m.on_key(vk, down, true, false, self.t);
            self.events.extend(d.events.iter().flatten().copied());
            d
        }
        fn wait(&mut self, ms: u64) {
            self.t += Duration::from_millis(ms);
            let (e, _) = self.m.tick(self.t);
            self.events.extend(e);
        }
    }

    #[test]
    fn hold_ctrl_win_activates_after_delay_and_masks_start() {
        let mut f = Feed::new();
        f.key(VK_LCONTROL, true);
        let d = f.key(VK_LWIN, true);
        assert!(d.arm_timer && !d.swallow);
        f.wait(60);
        assert!(f.events.is_empty());
        f.wait(70);
        assert_eq!(f.events, vec![HookEvent::ChordDown(ChordAction::Dictation)]);
        let d = f.key(VK_LWIN, false);
        assert!(d.mask_win);
        assert_eq!(
            f.events.last(),
            Some(&HookEvent::ChordUp(ChordAction::Dictation))
        );
        f.key(VK_LCONTROL, false);
    }

    #[test]
    fn windows_shortcut_is_not_hijacked() {
        // Ctrl+Win+D (new virtual desktop) pressed quickly.
        let mut f = Feed::new();
        f.key(VK_LCONTROL, true);
        f.key(VK_RWIN, true);
        f.wait(40);
        let d = f.key(D, true);
        assert!(!d.swallow);
        f.wait(200);
        assert!(
            f.events.is_empty(),
            "chord must be abandoned: {:?}",
            f.events
        );
        f.key(D, false);
        f.key(VK_RWIN, false);
        f.key(VK_LCONTROL, false);
        assert!(f.events.is_empty());
    }

    #[test]
    fn stray_key_taints_until_modifiers_released() {
        // Ctrl+C then Shift while Ctrl is still held must not arm Circle.
        let mut f = Feed::new();
        f.key(VK_LCONTROL, true);
        f.key(0x43, true);
        f.key(0x43, false);
        f.key(VK_LSHIFT, true);
        f.wait(300);
        assert!(f.events.is_empty());
        f.key(VK_LSHIFT, false);
        f.key(VK_LCONTROL, false);
        f.key(VK_RCONTROL, true);
        f.key(VK_RSHIFT, true);
        f.wait(130);
        assert_eq!(f.events, vec![HookEvent::ChordDown(ChordAction::Circle)]);
    }

    #[test]
    fn ctrl_alt_space_swallows_space_only() {
        let mut f = Feed::new();
        assert!(!f.key(VK_LCONTROL, true).swallow);
        assert!(!f.key(VK_LMENU, true).swallow);
        assert!(f.key(VK_SPACE, true).swallow);
        f.wait(130);
        assert!(f.key(VK_SPACE, true).swallow, "auto-repeat swallowed");
        assert!(f.key(VK_SPACE, false).swallow);
        assert_eq!(
            f.events,
            vec![
                HookEvent::ChordDown(ChordAction::Command),
                HookEvent::ChordUp(ChordAction::Command)
            ]
        );
        assert!(!f.key(VK_LMENU, false).swallow);
    }

    #[test]
    fn extra_modifier_cancels_match() {
        // Ctrl+Shift+Win matches no chord.
        let mut f = Feed::new();
        f.key(VK_LCONTROL, true);
        f.key(VK_LSHIFT, true);
        f.key(VK_LWIN, true);
        f.wait(200);
        assert!(f.events.is_empty());
    }

    #[test]
    fn releasing_early_fires_nothing() {
        let mut f = Feed::new();
        f.key(VK_LCONTROL, true);
        f.key(VK_LSHIFT, true);
        f.wait(50);
        f.key(VK_LSHIFT, false);
        f.wait(200);
        assert!(f.events.is_empty());
    }

    #[test]
    fn triple_escape_is_emergency_stop() {
        let mut f = Feed::new();
        for _ in 0..3 {
            let d = f.key(VK_ESCAPE, true);
            assert!(!d.swallow, "Esc is never swallowed");
            f.key(VK_ESCAPE, false);
            f.t += Duration::from_millis(300);
        }
        assert_eq!(f.events, vec![HookEvent::EmergencyStop]);
        let mut slow = Feed::new();
        for _ in 0..3 {
            slow.key(VK_ESCAPE, true);
            slow.key(VK_ESCAPE, false);
            slow.t += Duration::from_millis(900);
        }
        assert!(slow.events.is_empty());
    }

    #[test]
    fn paused_ignores_chords() {
        let mut f = Feed::new();
        f.m.paused = true;
        f.key(VK_LCONTROL, true);
        f.key(VK_LWIN, true);
        f.wait(200);
        assert!(f.events.is_empty());
    }

    #[test]
    fn physical_input_reported_only_when_asked() {
        let mut m = machine();
        let t = Instant::now();
        let d = m.on_key(A, true, true, true, t);
        assert_eq!(d.events[0], Some(HookEvent::PhysicalInput));
        let d = m.on_key(0x42, true, false, true, t);
        assert_eq!(
            d.events[0], None,
            "injected by other software is not physical"
        );
    }

    #[test]
    fn parse_rejects_single_modifier() {
        assert!(ChordDef::parse(ChordAction::Dictation, "ctrl").is_none());
        assert!(ChordDef::parse(ChordAction::Dictation, "ctrl+q").is_none());
        assert!(ChordDef::parse(ChordAction::Dictation, "ctrl+shift+space").is_some());
    }
}
