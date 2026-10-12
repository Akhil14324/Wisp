//! Media session (GSMTC), output volume and wallpaper (section 7.7).

use std::sync::mpsc;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
};

#[derive(Debug, Clone, Default, Serialize)]
pub struct MediaInfo {
    pub has_session: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// data: URL of the artwork, if any.
    pub artwork: Option<String>,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub app: String,
}

fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn manager() -> windows::core::Result<Manager> {
    Manager::RequestAsync()?.get()
}

fn read_artwork(s: &Session) -> Option<String> {
    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let thumb = props.Thumbnail().ok()?;
    let stream = thumb.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 4 * 1024 * 1024 {
        return None;
    }
    let mime = stream
        .ContentType()
        .map(|h| h.to_string())
        .unwrap_or_else(|_| "image/png".into());
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let mime = if mime.starts_with("image/") {
        mime
    } else {
        "image/png".into()
    };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf)
    ))
}

/// Current media session info (FR-ISL-05). Blocking.
pub fn current() -> MediaInfo {
    com_init();
    let Ok(m) = manager() else {
        return MediaInfo::default();
    };
    let Ok(s) = m.GetCurrentSession() else {
        return MediaInfo::default();
    };
    let mut info = MediaInfo {
        has_session: true,
        ..Default::default()
    };
    info.app = s
        .SourceAppUserModelId()
        .map(|h| h.to_string())
        .unwrap_or_default();
    if let Ok(p) = s.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
        info.title = p.Title().map(|h| h.to_string()).unwrap_or_default();
        info.artist = p.Artist().map(|h| h.to_string()).unwrap_or_default();
        info.album = p.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
    }
    if let Ok(pb) = s.GetPlaybackInfo() {
        info.playing = pb
            .PlaybackStatus()
            .map(|st| st == Status::Playing)
            .unwrap_or(false);
    }
    if let Ok(t) = s.GetTimelineProperties() {
        // TimeSpan is in 100 ns units.
        info.position_ms = t.Position().map(|d| d.Duration / 10_000).unwrap_or(0);
        let end = t.EndTime().map(|d| d.Duration / 10_000).unwrap_or(0);
        let start = t.StartTime().map(|d| d.Duration / 10_000).unwrap_or(0);
        info.duration_ms = (end - start).max(0);
    }
    info.artwork = read_artwork(&s);
    info
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    PlayPause,
    Next,
    Previous,
}

impl MediaAction {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "play_pause" => Some(Self::PlayPause),
            "next" => Some(Self::Next),
            "previous" => Some(Self::Previous),
            _ => None,
        }
    }
}

/// Controls the current session (media_control tool, island Music tab). Blocking.
pub fn control(action: MediaAction) -> anyhow::Result<()> {
    com_init();
    let s = manager()?
        .GetCurrentSession()
        .map_err(|_| anyhow::anyhow!("no media is playing"))?;
    let ok = match action {
        MediaAction::PlayPause => s.TryTogglePlayPauseAsync()?.get()?,
        MediaAction::Next => s.TrySkipNextAsync()?.get()?,
        MediaAction::Previous => s.TrySkipPreviousAsync()?.get()?,
    };
    if !ok {
        anyhow::bail!("the media app refused the request");
    }
    Ok(())
}

/// Calls `on_change` (debounced) whenever the session or its info changes.
pub fn watch(on_change: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("wisp-media".into())
        .spawn(move || {
            com_init();
            let (tx, rx) = mpsc::channel::<bool>();
            let Ok(m) = manager() else {
                tracing::warn!("media session manager unavailable");
                return;
            };
            let txm = tx.clone();
            let _ = m.CurrentSessionChanged(&windows::Foundation::TypedEventHandler::new(
                move |_, _| {
                    let _ = txm.send(true);
                    Ok(())
                },
            ));
            let mut session: Option<(Session, i64, i64, i64)> = None;
            let subscribe =
                |m: &Manager, tx: &mpsc::Sender<bool>| -> Option<(Session, i64, i64, i64)> {
                    let s = m.GetCurrentSession().ok()?;
                    let (a, b, c) = (tx.clone(), tx.clone(), tx.clone());
                    let t1 = s
                        .MediaPropertiesChanged(&windows::Foundation::TypedEventHandler::new(
                            move |_, _| {
                                let _ = a.send(false);
                                Ok(())
                            },
                        ))
                        .ok()?;
                    let t2 = s
                        .PlaybackInfoChanged(&windows::Foundation::TypedEventHandler::new(
                            move |_, _| {
                                let _ = b.send(false);
                                Ok(())
                            },
                        ))
                        .ok()?;
                    let t3 = s
                        .TimelinePropertiesChanged(&windows::Foundation::TypedEventHandler::new(
                            move |_, _| {
                                let _ = c.send(false);
                                Ok(())
                            },
                        ))
                        .ok()?;
                    Some((s, t1, t2, t3))
                };
            session = subscribe(&m, &tx).or(session);
            while let Ok(mut resub) = rx.recv() {
                // Debounce bursts of events.
                while let Ok(r) = rx.recv_timeout(Duration::from_millis(150)) {
                    resub |= r;
                }
                if resub {
                    if let Some((s, t1, t2, t3)) = session.take() {
                        let _ = s.RemoveMediaPropertiesChanged(t1);
                        let _ = s.RemovePlaybackInfoChanged(t2);
                        let _ = s.RemoveTimelinePropertiesChanged(t3);
                    }
                    session = subscribe(&m, &tx);
                }
                on_change();
            }
        })
        .expect("spawn media watcher");
}

fn endpoint_volume() -> anyhow::Result<IAudioEndpointVolume> {
    com_init();
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None)?;
        Ok(vol)
    }
}

/// Sets default output volume 0..=100 and/or mute (set_volume tool). Blocking.
pub fn set_volume(level: Option<u32>, mute: Option<bool>) -> anyhow::Result<()> {
    let vol = endpoint_volume()?;
    unsafe {
        if let Some(l) = level {
            vol.SetMasterVolumeLevelScalar(l.min(100) as f32 / 100.0, std::ptr::null())?;
            if mute.is_none() && l > 0 {
                vol.SetMute(false, std::ptr::null())?;
            }
        }
        if let Some(m) = mute {
            vol.SetMute(m, std::ptr::null())?;
        }
    }
    Ok(())
}

pub fn get_volume() -> anyhow::Result<(u32, bool)> {
    let vol = endpoint_volume()?;
    unsafe {
        let l = vol.GetMasterVolumeLevelScalar()?;
        let m = vol.GetMute()?.as_bool();
        Ok(((l * 100.0).round() as u32, m))
    }
}

pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "bmp"];

/// Sets the desktop wallpaper from a local image (set_wallpaper tool).
pub fn set_wallpaper(path: &str) -> anyhow::Result<()> {
    let p = std::path::Path::new(path.trim());
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        anyhow::bail!("only .jpg, .png and .bmp images can be used");
    }
    let abs =
        std::fs::canonicalize(p).map_err(|_| anyhow::anyhow!("that image file does not exist"))?;
    if !abs.is_file() {
        anyhow::bail!("that path is not a file");
    }
    let s = abs
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide.as_ptr() as *mut core::ffi::c_void),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )?;
    }
    Ok(())
}
