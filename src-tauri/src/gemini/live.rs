//! Gemini Live API streaming path (section 5.6, milestone M8).
//!
//! M8 is optional and starts only after M7 passes on the user's PC, so v1 ships
//! the push-to-talk request path only. See docs/DEVIATIONS.md.

/// Whether the streaming command path is available in this build.
pub const AVAILABLE: bool = false;
