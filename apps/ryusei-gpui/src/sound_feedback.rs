//! UI-local sound feedback boundary.
//!
//! Sound is intentionally outside host transactions: a failed mutation must
//! never emit feedback, and persistence/recovery must stay deterministic.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoundCue {
    StonePlaced,
    Pass,
}

pub trait SoundSink: 'static {
    fn play(&mut self, cue: SoundCue);
}

/// Explicitly silent fallback for CI, unsupported platforms and deployments
/// where a best-effort platform process cannot be started.
#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub struct NoopSoundSink;

#[cfg(not(target_os = "macos"))]
impl SoundSink for NoopSoundSink {
    fn play(&mut self, _: SoundCue) {}
}

/// Returns the platform feedback backend without making audio device setup part
/// of application startup. Unsupported platforms stay silent by design.
pub fn platform_sound_sink() -> Box<dyn SoundSink> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacosSoundSink)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::<NoopSoundSink>::default()
    }
}

/// macOS system-sound sink. `afplay` is available on supported releases and
/// receives only an absolute Apple-owned asset path. Playback runs detached so
/// a sound device hiccup can never block a GPUI event handler.
#[cfg(target_os = "macos")]
pub struct MacosSoundSink;

#[cfg(target_os = "macos")]
impl SoundSink for MacosSoundSink {
    fn play(&mut self, cue: SoundCue) {
        let asset = match cue {
            SoundCue::StonePlaced => "/System/Library/Sounds/Tink.aiff",
            SoundCue::Pass => "/System/Library/Sounds/Pop.aiff",
        };
        let _ = std::process::Command::new("/usr/bin/afplay")
            .arg(asset)
            .spawn();
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct RecordingSoundSink {
    cues: Vec<SoundCue>,
}

#[cfg(test)]
impl RecordingSoundSink {
    pub fn cues(&self) -> &[SoundCue] {
        &self.cues
    }
}

#[cfg(test)]
impl SoundSink for RecordingSoundSink {
    fn play(&mut self, cue: SoundCue) {
        self.cues.push(cue);
    }
}

pub fn play_if_enabled(
    settings: &ryusei_host::SettingsStore,
    sink: &mut dyn SoundSink,
    cue: SoundCue,
) {
    if settings.get_bool("sound.enable").unwrap_or(true) {
        sink.play(cue);
    }
}

#[cfg(test)]
mod tests {
    use super::{RecordingSoundSink, SoundCue, platform_sound_sink, play_if_enabled};
    use ryusei_host::SettingsStore;
    use serde_json::json;

    #[test]
    fn platform_sink_is_available_without_audio_initialization() {
        let mut sink = platform_sound_sink();
        // This is deliberately best-effort: construction must work in CI and
        // missing audio devices must not surface through game interaction.
        sink.play(SoundCue::StonePlaced);
    }

    #[test]
    fn emits_only_when_sound_is_enabled() {
        let mut settings = SettingsStore::default();
        let mut sink = RecordingSoundSink::default();
        play_if_enabled(&settings, &mut sink, SoundCue::StonePlaced);
        assert_eq!(sink.cues(), &[SoundCue::StonePlaced]);

        settings.set("sound.enable", json!(false)).unwrap();
        play_if_enabled(&settings, &mut sink, SoundCue::Pass);
        assert_eq!(sink.cues(), &[SoundCue::StonePlaced]);
    }
}
