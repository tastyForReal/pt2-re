use super::midi_types::{MidiJson, midi_to_note, ticks_to_seconds};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[cfg(feature = "audio")]
use rodio::{Decoder, OutputStream, OutputStreamHandle, Source};
#[cfg(feature = "audio")]
use std::sync::Arc;
#[cfg(feature = "audio")]
use std::time::Duration;

// Soundfont imports - conditionally compiled
#[cfg(feature = "soundfont")]
use super::soundfont_manager::{
    ArcSoundfontSynth, SharedTsfHandle, SoundfontConfig, SoundfontError, SoundfontResult,
    SoundfontSource, SoundfontState, SoundfontSynth, TinySFSynth, validate_soundfont_path,
};

/// Internal representation of a pre-decoded audio sample.
#[cfg(feature = "audio")]
struct DecodedSample {
    data: Arc<[f32]>,
    channels: u16,
    sample_rate: u32,
}

#[cfg(feature = "audio")]
struct SharedSource {
    data: Arc<[f32]>,
    channels: u16,
    sample_rate: u32,
    pos: usize,
}

#[cfg(feature = "audio")]
impl Iterator for SharedSource {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.data.len() {
            let sample = self.data[self.pos];
            self.pos += 1;
            Some(sample)
        } else {
            None
        }
    }
}

#[cfg(feature = "audio")]
impl Source for SharedSource {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.data.len() - self.pos)
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        let frames = self.data.len() / self.channels as usize;
        Some(Duration::from_secs_f64(
            frames as f64 / self.sample_rate as f64,
        ))
    }
}

/// Wrapper around DynamicMixer that never returns None.
/// rodio's DynamicMixer returns None when there are no active sources,
/// which causes the Sink to consider the source "finished" and stop polling.
/// This wrapper returns silence (0.0) instead, keeping the Sink alive.
#[cfg(feature = "audio")]
struct InfiniteMixerSource {
    inner: rodio::dynamic_mixer::DynamicMixer<f32>,
    channels: u16,
    sample_rate: u32,
}

#[cfg(feature = "audio")]
impl Iterator for InfiniteMixerSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        // Always return Some — produce silence when the inner mixer has no sources
        Some(self.inner.next().unwrap_or(0.0))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, None)
    }
}

#[cfg(feature = "audio")]
impl Source for InfiniteMixerSource {
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.channels
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Audio manager: tracks MIDI-derived sounds and playback state.
/// Tracks a note that has been triggered (note_on) and is awaiting note_off.
struct ActiveNote {
    midi_number: u8,
    end_time: f64,
}

pub struct AudioManager {
    midi_data: Option<MidiJson>,
    track_pointers: Vec<usize>,
    played_notes: HashSet<i64>,
    active_notes: Vec<ActiveNote>,
    /// When true, all audio output is suppressed (used during video recording).
    muted: bool,

    #[cfg(feature = "audio")]
    _stream: Option<OutputStream>,
    #[cfg(feature = "audio")]
    handle: Option<OutputStreamHandle>,
    #[cfg(feature = "audio")]
    samples: Arc<std::sync::RwLock<HashMap<String, DecodedSample>>>,
    #[cfg(feature = "audio")]
    sample_names: Vec<String>,
    #[cfg(feature = "audio")]
    assets_dir: Option<String>,
    #[cfg(feature = "audio")]
    mixer_controller: Option<Arc<rodio::dynamic_mixer::DynamicMixerController<f32>>>,
    #[cfg(feature = "audio")]
    _mixer_sink: Option<rodio::Sink>,

    // === Soundfont synthesis fields ===
    #[cfg(feature = "soundfont")]
    soundfont_synth: Option<Box<dyn SoundfontSynth>>,
    #[cfg(feature = "soundfont")]
    soundfont_state: SoundfontState,
    #[cfg(feature = "soundfont")]
    soundfont_enabled: bool,
    #[cfg(feature = "soundfont")]
    soundfont_fallback_enabled: bool,
    #[cfg(feature = "soundfont")]
    using_soundfont: bool,
    #[cfg(feature = "soundfont")]
    soundfont_path: Option<String>,
    #[cfg(feature = "soundfont")]
    soundfont_playback_active: bool,
    #[cfg(feature = "soundfont")]
    soundfont_synth_arc: Option<SharedTsfHandle>,
    #[cfg(feature = "soundfont")]
    soundfont_sample_rate: u32,
}

/// Create a mixer + infinite wrapper and attach to a Sink.
/// Returns (controller, sink) or None if creation fails.
#[cfg(feature = "audio")]
fn create_mixer_and_sink(
    handle: &OutputStreamHandle,
) -> Option<(
    Arc<rodio::dynamic_mixer::DynamicMixerController<f32>>,
    rodio::Sink,
)> {
    let (controller, mixer_source) = rodio::dynamic_mixer::mixer::<f32>(2, 44100);
    let infinite_source = InfiniteMixerSource {
        inner: mixer_source,
        channels: 2,
        sample_rate: 44100,
    };
    if let Ok(sink) = rodio::Sink::try_new(handle) {
        sink.append(infinite_source);
        Some((controller, sink))
    } else {
        None
    }
}

impl AudioManager {
    pub fn new() -> Self {
        #[cfg(feature = "audio")]
        let (stream, handle) = match OutputStream::try_default() {
            Ok((s, h)) => (Some(s), Some(h)),
            Err(e) => {
                log::error!("Failed to initialize rodio output stream: {}", e);
                (None, None)
            }
        };

        #[cfg(feature = "audio")]
        let (mixer_controller, mixer_sink) = if let Some(ref h) = handle {
            match create_mixer_and_sink(h) {
                Some((c, s)) => (Some(c), Some(s)),
                None => (None, None),
            }
        } else {
            (None, None)
        };

        Self {
            midi_data: None,
            track_pointers: Vec::new(),
            played_notes: HashSet::new(),
            active_notes: Vec::new(),
            muted: false,

            #[cfg(feature = "audio")]
            _stream: stream,
            #[cfg(feature = "audio")]
            handle,
            #[cfg(feature = "audio")]
            samples: Arc::new(std::sync::RwLock::new(HashMap::new())),
            #[cfg(feature = "audio")]
            sample_names: Vec::new(),
            #[cfg(feature = "audio")]
            assets_dir: None,
            #[cfg(feature = "audio")]
            mixer_controller,
            #[cfg(feature = "audio")]
            _mixer_sink: mixer_sink,

            // === Soundfont fields (initialized to defaults) ===
            #[cfg(feature = "soundfont")]
            soundfont_synth: None,
            #[cfg(feature = "soundfont")]
            soundfont_state: SoundfontState::Uninitialized,
            #[cfg(feature = "soundfont")]
            soundfont_enabled: true,
            #[cfg(feature = "soundfont")]
            soundfont_fallback_enabled: true,
            #[cfg(feature = "soundfont")]
            using_soundfont: false,
            #[cfg(feature = "soundfont")]
            soundfont_path: None,
            #[cfg(feature = "soundfont")]
            soundfont_playback_active: false,
            #[cfg(feature = "soundfont")]
            soundfont_synth_arc: None,
            #[cfg(feature = "soundfont")]
            soundfont_sample_rate: 44100,
        }
    }

    pub fn initialize_samples(&mut self, assets_dir: &str, preload: bool) {
        #[cfg(feature = "audio")]
        {
            self.assets_dir = Some(assets_dir.to_string());
            let piano_path = Path::new(assets_dir).join("sounds/mp3/piano");

            // Collect all MP3 file paths first
            let mp3_files: Vec<(String, std::path::PathBuf)> =
                if let Ok(entries) = std::fs::read_dir(&piano_path) {
                    entries
                        .flatten()
                        .filter_map(|entry| {
                            let name = entry.file_name().to_str()?.to_string();
                            if name.ends_with(".mp3") {
                                Some((name, entry.path()))
                            } else {
                                None
                            }
                        })
                        .collect()
                } else {
                    Vec::new()
                };

            self.sample_names = mp3_files.iter().map(|(n, _)| n.clone()).collect();

            if preload {
                log::info!("Loading and decoding audio samples from: {:?}", piano_path);
                // Decode all samples in parallel using scoped threads
                let results: Vec<(String, DecodedSample)> = std::thread::scope(|s| {
                    let handles: Vec<_> = mp3_files
                        .iter()
                        .map(|(name, path)| {
                            let name = name.clone();
                            let path = path.clone();
                            s.spawn(move || -> Option<(String, DecodedSample)> {
                                let data = std::fs::read(&path).ok()?;
                                let cursor = std::io::Cursor::new(data);
                                let decoder = Decoder::new(cursor).ok()?;
                                // Normalize to stereo and 44100Hz using UniformSourceIterator (matches Mixer)
                                // We avoid .collect() here because of a bug in rodio 0.20.1's ChannelCountConverter::size_hint
                                // which can cause a subtraction overflow panic during Vec pre-allocation.
                                let uniform_source = rodio::source::UniformSourceIterator::new(
                                    decoder.convert_samples::<f32>(),
                                    2,
                                    44100,
                                );
                                let mut decoded_data = Vec::new();
                                for sample in uniform_source {
                                    decoded_data.push(sample);
                                }
                                Some((
                                    name,
                                    DecodedSample {
                                        data: Arc::from(decoded_data),
                                        channels: 2,
                                        sample_rate: 44100,
                                    },
                                ))
                            })
                        })
                        .collect();

                    handles
                        .into_iter()
                        .filter_map(|h| h.join().ok().flatten())
                        .collect()
                });

                let mut sample_map = self.samples.write().unwrap();
                for (name, sample) in results {
                    sample_map.insert(name, sample);
                }

                log::info!("Loaded and pre-decoded {} audio samples", sample_map.len());
            } else {
                log::info!(
                    "Discovered {} audio samples for streaming",
                    self.sample_names.len()
                );
            }
        }
    }

    // =====================================================================
    // Soundfont Management Methods (soundfont feature)
    // =====================================================================

    #[cfg(feature = "soundfont")]
    /// Initialize soundfont synthesis with the given path.
    ///
    /// # Arguments
    /// * `path` - Path to the soundfont file (.sf2)
    /// * `fallback_enabled` - Whether to fall back to MP3 if soundfont fails
    ///
    /// # Returns
    /// * `Ok(())` - Soundfont loaded successfully
    /// * `Err(SoundfontError)` - Failed to load soundfont
    ///
    /// # Example
    /// ```ignore
    /// // Load default soundfont with fallback enabled
    /// match audio_manager.load_soundfont("assets/sounds/sf2/piano.sf2", true) {
    ///     Ok(()) => println!("Soundfont ready!"),
    ///     Err(e) => println!("Failed to load soundfont: {}", e),
    /// }
    /// ```
    pub fn load_soundfont(&mut self, path: &str, fallback_enabled: bool) -> SoundfontResult<()> {
        self.soundfont_fallback_enabled = fallback_enabled;

        // Validate the path first
        let validated_path = match validate_soundfont_path(path) {
            Ok(p) => p,
            Err(e) => {
                self.handle_soundfont_error(&e);
                return Err(e);
            }
        };

        let path_str = validated_path.to_string_lossy().to_string();
        log::info!("Loading soundfont from: {}", path_str);

        // Create new synthesizer
        let mut synth = match TinySFSynth::new() {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to create synthesizer: {}", e);
                return Err(e);
            }
        };

        // Configure and load soundfont
        let config = SoundfontConfig::new(&path_str)
            .with_sample_rate(44100)
            .with_voices(256)
            .with_volume(0.5);

        match synth.load_soundfont(config) {
            Ok(()) => {
                // Create SoundfontSource to feed audio to the mixer
                let source = SoundfontSource::from_tinysf(synth);

                // Clone the synth arc before the source is moved into the mixer
                let synth_arc = source.clone_synth_arc();

                // Store the synth arc and sample rate so we can recreate the
                // SoundfontSource if the mixer is rebuilt (e.g. stop_all_samples).
                self.soundfont_synth_arc = Some(synth_arc.clone());
                self.soundfont_sample_rate = 44100;

                // Wrap in ArcSoundfontSynth for the trait object
                let wrapped_synth = ArcSoundfontSynth::new(synth_arc);
                self.soundfont_synth = Some(Box::new(wrapped_synth));

                // Add the source to the mixer if available
                if let Some(ref controller) = self.mixer_controller {
                    controller.add(source);
                    log::info!("Soundfont source added to mixer");
                }

                self.soundfont_state = SoundfontState::Ready;
                self.using_soundfont = true;
                self.soundfont_path = Some(path_str);
                self.soundfont_playback_active = true;

                log::info!("Soundfont loaded successfully");
                Ok(())
            }
            Err(e) => {
                self.handle_soundfont_error(&e);
                Err(e)
            }
        }
    }

    #[cfg(feature = "soundfont")]
    fn handle_soundfont_error(&mut self, error: &SoundfontError) {
        self.soundfont_state = SoundfontState::Fallback;
        self.using_soundfont = false;

        match error {
            SoundfontError::FileNotFound(path) => {
                log::error!("Soundfont file not found: {}", path);
            }
            SoundfontError::ParseError(msg) => {
                log::error!("Invalid soundfont format: {}", msg);
            }
            SoundfontError::SynthInitError(msg) => {
                log::error!("Synthesizer initialization failed: {}", msg);
            }
            SoundfontError::NoPresets(path) => {
                log::error!("Soundfont has no presets: {}", path);
            }
            _ => {
                log::error!("Soundfont error: {}", error);
            }
        }

        if self.soundfont_fallback_enabled {
            log::info!("Falling back to MP3 sample playback");
        } else {
            log::warn!("Fallback disabled; audio may not play");
        }
    }

    #[cfg(feature = "soundfont")]
    #[allow(dead_code)]
    /// Start soundfont audio playback through the mixer.
    /// This must be called after successfully loading a soundfont to route its audio output.
    fn start_soundfont_playback(&mut self) {
        if let Some(ref mut _synth_box) = self.soundfont_synth {
            // This requires restructuring - for now we handle it in load_soundfont
            // where we have access to the raw TinySFSynth
            log::debug!("Soundfont playback routing not yet implemented");
        }
        self.soundfont_playback_active = true;
    }

    #[cfg(feature = "soundfont")]
    /// Enable or disable soundfont synthesis at runtime.
    pub fn set_soundfont_enabled(&mut self, enabled: bool) {
        self.soundfont_enabled = enabled;

        if !enabled && self.using_soundfont {
            log::info!("Soundfont synthesis disabled by user");
            self.using_soundfont = false;
        }

        log::info!("Soundfont enabled setting: {}", enabled);
    }

    #[cfg(feature = "soundfont")]
    /// Reload soundfont from a different path.
    /// Unloads current soundfont first, then loads the new one.
    pub fn reload_soundfont(&mut self, new_path: &str) -> SoundfontResult<()> {
        log::info!("Reloading soundfont from: {}", new_path);

        // Unload existing
        self.unload_soundfont();

        // Load new
        self.load_soundfont(new_path, self.soundfont_fallback_enabled)
    }

    #[cfg(feature = "soundfont")]
    /// Unload soundfont and release resources.
    /// Safe to call even if no soundfont is loaded.
    pub fn unload_soundfont(&mut self) {
        if let Some(ref synth) = self.soundfont_synth {
            log::info!("Unloading soundfont...");
            synth.reset();
            synth.reset();
        }

        self.soundfont_synth = None;
        self.soundfont_synth_arc = None;
        self.soundfont_state = SoundfontState::Uninitialized;
        self.using_soundfont = false;
        self.soundfont_path = None;

        log::info!("Soundfont unloaded");
    }

    #[cfg(feature = "soundfont")]
    /// Re-add the soundfont source to the current mixer.
    /// This must be called after the mixer is recreated (e.g. in stop_all_samples)
    /// to restore audio output from the synthesizer.
    fn readd_soundfont_to_mixer(&mut self) {
        if !self.using_soundfont || !self.soundfont_enabled {
            return;
        }
        let synth_arc = match &self.soundfont_synth_arc {
            Some(arc) => arc.clone(),
            None => return,
        };
        if let Some(ref controller) = self.mixer_controller {
            let source = SoundfontSource::from_synth_arc(synth_arc, self.soundfont_sample_rate);
            controller.add(source);
            log::debug!("Soundfont source re-added to mixer after rebuild");
        }
    }

    #[cfg(feature = "soundfont")]
    /// Get current soundfont state for diagnostics.
    pub fn soundfont_state(&self) -> SoundfontState {
        #[cfg(feature = "soundfont")]
        {
            self.soundfont_state.clone()
        }
        #[cfg(not(feature = "soundfont"))]
        {
            // Return a static value when soundfont is not compiled
            SoundfontState::Disabled
        }
    }

    #[cfg(feature = "soundfont")]
    /// Check if soundfont is currently active (loaded and enabled).
    pub fn is_using_soundfont(&self) -> bool {
        self.using_soundfont && self.soundfont_enabled
    }

    #[cfg(feature = "soundfont")]
    /// Get diagnostic information about soundfont state.
    pub fn get_soundfont_info(&self) -> String {
        if !cfg!(feature = "soundfont") {
            return "Soundfont support not compiled in".to_string();
        }

        format!(
            "Soundfont: state={:?}, enabled={}, fallback={}, active={}, path={:?}",
            self.soundfont_state,
            self.soundfont_enabled,
            self.soundfont_fallback_enabled,
            self.using_soundfont,
            self.soundfont_path
        )
    }

    // =====================================================================
    // Non-feature-gated stubs for API compatibility
    // =====================================================================

    #[cfg(not(feature = "soundfont"))]
    /// Stub for non-soundfont builds - always returns error.
    pub fn load_soundfont(&mut self, _path: &str, _fallback_enabled: bool) -> Result<(), String> {
        Err("Soundfont support not compiled in (enable 'soundfont' feature)".to_string())
    }

    #[cfg(not(feature = "soundfont"))]
    pub fn set_soundfont_enabled(&mut self, _enabled: bool) {}

    #[cfg(not(feature = "soundfont"))]
    pub fn reload_soundfont(&mut self, _path: &str) -> Result<(), String> {
        Err("Soundfont support not compiled in".to_string())
    }

    #[cfg(not(feature = "soundfont"))]
    pub fn unload_soundfont(&mut self) {}

    #[cfg(not(feature = "soundfont"))]
    pub fn soundfont_state(&self) -> &'static str {
        "Disabled"
    }

    #[cfg(not(feature = "soundfont"))]
    pub fn is_using_soundfont(&self) -> bool {
        false
    }

    #[cfg(not(feature = "soundfont"))]
    pub fn get_soundfont_info(&self) -> String {
        "Soundfont support not compiled in".to_string()
    }

    // =====================================================================
    // End Soundfont Methods
    // =====================================================================

    pub fn load_midi_data(&mut self, midi_data: MidiJson) {
        self.midi_data = Some(midi_data);
        self.reset_playback();
    }

    pub fn clear_midi_data(&mut self) {
        self.midi_data = None;
        self.reset_playback();
    }

    pub fn reset_playback(&mut self) {
        self.track_pointers = vec![0; self.midi_data.as_ref().map(|m| m.tracks.len()).unwrap_or(0)];
        self.played_notes.clear();
        self.active_notes.clear();
        self.stop_all_samples();

        #[cfg(feature = "soundfont")]
        {
            // Stop any playing notes on the synthesizer
            if let Some(ref synth) = self.soundfont_synth {
                synth.reset();
            }
        }
    }

    /// Mute or unmute all audio output.
    /// When muted, `play_sample`, `play_note_by_midi`, `update_midi_playback`,
    /// and other output methods become no-ops.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if muted {
            log::info!("Audio output muted");
            // Stop any currently playing samples immediately
            self.stop_all_samples();
            #[cfg(feature = "soundfont")]
            if let Some(ref synth) = self.soundfont_synth {
                synth.reset();
            }
        } else {
            log::info!("Audio output unmuted");
        }
    }

    /// Returns whether audio output is currently muted.
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn update_midi_playback(
        &mut self,
        current_time: f64,
        _skipped_note_ids: &[i64],
        speed_multiplier: f64,
    ) -> Vec<i64> {
        if self.muted {
            return Vec::new();
        }
        let midi_data = match &self.midi_data {
            Some(m) => m,
            None => return Vec::new(),
        };

        // Collect header info for tick-based duration conversion
        let ppq = midi_data.header.ppq;
        let tempos = &midi_data.header.tempos;

        let mut played_note_ids = Vec::new();
        let mut notes_to_play = Vec::new();

        for track_idx in 0..midi_data.tracks.len() {
            let track = &midi_data.tracks[track_idx];
            let pointer = self.track_pointers.get_mut(track_idx).unwrap();

            while *pointer < track.notes.len() {
                let note = &track.notes[*pointer];
                if note.time > current_time {
                    break;
                }

                let lookback_window = 2.0;

                if note.time > current_time - lookback_window {
                    let note_id = (note.time * 1000.0).round() as i64 * 1_000_000
                        + track_idx as i64 * 1000
                        + note.midi as i64;

                    if !self.played_notes.contains(&note_id) {
                        let is_skipped = _skipped_note_ids.contains(&note_id);
                        if !is_skipped && note.midi >= 21 && note.midi <= 108 {
                            notes_to_play.push(note.midi);

                            // Tick-based note_off scheduling:
                            // Use duration_ticks (raw tick count) converted to seconds
                            // via ticks_to_seconds for accurate tempo-aware duration,
                            // then scale by speed_multiplier for playback sync.
                            let end_time = if note.duration_ticks > 0 {
                                let end_tick = note.ticks + note.duration_ticks;
                                let duration_in_secs =
                                    ticks_to_seconds(end_tick, tempos, ppq) - note.time;
                                // Scale by speed_multiplier so that the soundfont note
                                // sustains for its original real-time duration regardless
                                // of how fast the MIDI clock advances.
                                let scaled_duration = if speed_multiplier > 0.0 {
                                    duration_in_secs * speed_multiplier
                                } else {
                                    duration_in_secs
                                };
                                note.time + scaled_duration
                            } else {
                                // Fallback for dynamic notes without tick data
                                let scaled_duration = if speed_multiplier > 0.0 {
                                    note.duration * speed_multiplier
                                } else {
                                    note.duration
                                };
                                note.time + scaled_duration
                            };

                            if note.duration > 0.0 || note.duration_ticks > 0 {
                                self.active_notes.push(ActiveNote {
                                    midi_number: note.midi,
                                    end_time,
                                });
                            }
                        }

                        self.played_notes.insert(note_id);
                        played_note_ids.push(note_id);
                    }
                }
                *pointer += 1;
            }
        }

        // Send note_off for notes that have reached their end time
        let mut i = 0;
        while i < self.active_notes.len() {
            if current_time >= self.active_notes[i].end_time {
                let note = self.active_notes.remove(i);
                self.play_note_off_by_midi(note.midi_number);
            } else {
                i += 1;
            }
        }

        for midi in notes_to_play {
            self.play_note_by_midi(midi);
        }

        // Cleanup old played notes
        let max_note_time = current_time - 10.0;
        self.played_notes.retain(|&note_id| {
            let note_time = (note_id / 1_000_000) as f64 / 1000.0;
            note_time >= max_note_time
        });

        played_note_ids
    }

    pub fn play_note_by_midi(&self, midi_number: u8) {
        if self.muted {
            return;
        }
        #[cfg(feature = "soundfont")]
        {
            // Check if soundfont is available and enabled
            if self.using_soundfont
                && self.soundfont_enabled
                && let Some(ref synth) = self.soundfont_synth
            {
                // Use velocity based on note or default to 96
                let velocity = 96u8;
                synth.note_on(0, midi_number, velocity);
                log::debug!("Soundfont note ON: midi={}", midi_number);
                return;
            }
        }

        // Fallback to MP3 sample playback
        if let Some(note_name) = midi_to_note(midi_number) {
            let filename = format!("{}.mp3", note_name);
            self.play_sample(&filename);
        }
    }

    /// Send a note_off event for the given MIDI number.
    /// This is primarily important for soundfont synthesis, which sustains notes
    /// until explicitly released, allowing the release envelope to play.
    pub fn play_note_off_by_midi(&self, midi_number: u8) {
        if self.muted {
            return;
        }
        #[cfg(feature = "soundfont")]
        {
            if self.using_soundfont
                && self.soundfont_enabled
                && let Some(ref synth) = self.soundfont_synth
            {
                synth.note_off(0, midi_number);
                log::debug!("Soundfont note OFF: midi={}", midi_number);
            }
        }
    }

    /// Clear all tracked active notes, sending proper note_off for each.
    /// This should be called when a song loop transitions to prevent stale
    /// note_off events from the previous loop from killing notes in the new loop.
    pub fn clear_active_notes(&mut self) {
        // Send release for every active note (important for soundfont synthesis
        // to play proper release envelopes rather than hard-cutting)
        for note in &self.active_notes {
            self.play_note_off_by_midi(note.midi_number);
        }
        if !self.active_notes.is_empty() {
            log::debug!(
                "Cleared {} active notes on loop transition",
                self.active_notes.len()
            );
        }
        self.active_notes.clear();
    }

    pub fn play_sample(&self, filename: &str) {
        if self.muted {
            return;
        }
        #[cfg(feature = "audio")]
        {
            if let Some(controller) = &self.mixer_controller {
                // First check if the sample is already loaded (lazy cache or pre-loaded)
                let is_loaded = self.samples.read().unwrap().contains_key(filename);

                // If not loaded, but we have the assets directory (lazy loading mode)
                if !is_loaded {
                    if let Some(ref dir) = self.assets_dir {
                        let path = Path::new(dir).join("sounds/mp3/piano").join(filename);
                        let filename_string = filename.to_string();
                        let controller_clone = Arc::clone(controller);
                        let samples_clone = Arc::clone(&self.samples);

                        // Spawn a detached thread to handle file IO and decoding so we NEVER block the main thread.
                        std::thread::spawn(move || {
                            if let Ok(file) = std::fs::File::open(&path) {
                                let reader = std::io::BufReader::new(file);
                                if let Ok(decoder) = Decoder::new(reader) {
                                    log::debug!(
                                        "Lazy loading and decoding audio sample in background: {}",
                                        filename_string
                                    );
                                    let uniform_source = rodio::source::UniformSourceIterator::new(
                                        decoder.convert_samples::<f32>(),
                                        2,
                                        44100,
                                    );
                                    let mut decoded_data = Vec::new();
                                    for sample in uniform_source {
                                        decoded_data.push(sample);
                                    }

                                    let decoded_sample = DecodedSample {
                                        data: Arc::from(decoded_data),
                                        channels: 2,
                                        sample_rate: 44100,
                                    };

                                    // Instantly play the sample from the newly decoded arc without waiting for locking cycles
                                    let source = SharedSource {
                                        data: Arc::clone(&decoded_sample.data),
                                        channels: decoded_sample.channels,
                                        sample_rate: decoded_sample.sample_rate,
                                        pos: 0,
                                    };
                                    controller_clone.add(source);

                                    // Cache it safely on write lock
                                    if let Ok(mut map) = samples_clone.write() {
                                        map.insert(filename_string, decoded_sample);
                                    }
                                } else {
                                    log::warn!(
                                        "Failed to decode lazily loaded file: {}",
                                        filename_string
                                    );
                                }
                            } else {
                                log::warn!("Lazy load sample file not found: {}", filename_string);
                            }
                        });
                        return; // Return early, the background thread will push this to the controller directly.
                    } else {
                        log::warn!("Sample not found and no assets dir: {}", filename);
                    }
                }

                // If the sample exists (either from pre-load or a prior lazy-load), play it securely as zero-copy Arc.
                if let Ok(sample_map) = self.samples.read()
                    && let Some(sample) = sample_map.get(filename)
                {
                    log::debug!("Playing audio sample: {}", filename);
                    let source = SharedSource {
                        data: Arc::clone(&sample.data),
                        channels: sample.channels,
                        sample_rate: sample.sample_rate,
                        pos: 0,
                    };
                    controller.add(source);
                }
            }
        }
    }

    pub fn play_random_sample(&self) {
        if self.muted {
            return;
        }
        #[cfg(feature = "audio")]
        {
            if self.midi_data.is_some() {
                return;
            }

            if let Some(_handle) = &self.handle {
                if !self.sample_names.is_empty() {
                    use rand::Rng;
                    let mut rng = rand::thread_rng();
                    let random_index = rng.gen_range(0..self.sample_names.len());
                    if let Some(filename) = self.sample_names.get(random_index) {
                        log::debug!("Playing random sample: {}", filename);
                        self.play_sample(filename);
                    }
                } else {
                    log::warn!("No samples available to play random sound");
                }
            }
        }
    }

    pub fn play_game_over_chord(&self) {
        if self.muted {
            return;
        }
        self.play_sample("c.mp3");
        self.play_sample("e.mp3");
        self.play_sample("g.mp3");
    }

    pub fn add_dynamic_midi_note(&mut self, track_idx: usize, midi: u8, time: f64, duration: f64) {
        if let Some(ref mut midi_data) = self.midi_data
            && let Some(track) = midi_data.tracks.get_mut(track_idx)
        {
            track.notes.push(super::midi_types::MidiNote {
                midi,
                name: None,
                ticks: 0,
                time,
                duration,
                duration_ticks: 0,
                velocity: 1.0,
                note_off_velocity: 0.0,
            });
        }
    }

    pub fn stop_all_samples(&mut self) {
        #[cfg(feature = "audio")]
        {
            // Resetting the mixer is the easiest way to stop all sounds.
            if let (Some(handle), Some(old_sink)) = (&self.handle, &mut self._mixer_sink) {
                old_sink.stop();
                if let Some((controller, sink)) = create_mixer_and_sink(handle) {
                    self.mixer_controller = Some(controller);
                    self._mixer_sink = Some(sink);
                }
            }
        }

        #[cfg(feature = "soundfont")]
        {
            // Stop all synthesizer notes
            if let Some(ref synth) = self.soundfont_synth {
                synth.reset();
            }

            // Re-add the soundfont source to the newly created mixer.
            // Without this, note_on() events fire but no audio is produced
            // because the SoundfontSource that feeds the mixer was destroyed.
            self.readd_soundfont_to_mixer();
        }
    }
}

#[cfg(feature = "soundfont")]
impl Drop for AudioManager {
    fn drop(&mut self) {
        self.unload_soundfont();
        log::debug!("AudioManager dropped (soundfont cleanup complete)");
    }
}
