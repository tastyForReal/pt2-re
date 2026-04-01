//! Soundfont synthesis management module.
//!
//! This module provides safe Rust bindings to fluidlite for software synthesizer
//! playback using SoundFont (.sf2) files. It integrates with the existing rodio
//! audio pipeline via the DynamicMixer architecture.
//!
//! # Features
//! - Lazy loading of soundfont files
//! - MIDI note playback (note_on/note_off)
//! - Configurable polyphony and effects
//! - Thread-safe access from game loop
//! - Automatic fallback support
//!
//! # Example
//! ```ignore
//! let config = SoundfontConfig::default();
//! let mut synth = FluidLiteSynth::new();
//! synth.load_soundfont(&config)?;
//! synth.note_on(0, 60, 100);  // Play middle C
//! ```

use std::path::Path;

#[cfg(feature = "soundfont")]
use std::sync::Arc;

/// Result type for soundfont operations
pub type SoundfontResult<T> = Result<T, SoundfontError>;

/// Soundfont-specific errors with context
#[derive(Debug, Clone)]
pub enum SoundfontError {
    /// Soundfont file not found at specified path
    FileNotFound(String),
    /// Failed to read soundfont file (permissions, I/O error)
    FileReadError(String),
    /// Soundfont file is corrupted or invalid format
    ParseError(String),
    /// Failed to initialize the audio synthesizer
    SynthInitError(String),
    /// No presets found in soundfont
    NoPresets(String),
    /// Soundfont support not compiled in
    NotEnabled,
    /// Already loaded (need to unload first)
    AlreadyLoaded,
    /// Not loaded (cannot perform operation)
    NotLoaded,
}

impl std::fmt::Display for SoundfontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotFound(path) => {
                write!(f, "Soundfont file not found at: {}", path)
            }
            Self::FileReadError(path) => {
                write!(f, "Failed to read soundfont file: {}", path)
            }
            Self::ParseError(msg) => {
                write!(f, "Failed to parse soundfont: {}", msg)
            }
            Self::SynthInitError(msg) => {
                write!(f, "Failed to initialize synthesizer: {}", msg)
            }
            Self::NoPresets(path) => {
                write!(f, "Soundfont has no presets: {}", path)
            }
            Self::NotEnabled => {
                write!(
                    f,
                    "Soundfont support not compiled in (enable 'soundfont' feature)"
                )
            }
            Self::AlreadyLoaded => {
                write!(f, "Soundfont already loaded; unload first before reloading")
            }
            Self::NotLoaded => {
                write!(f, "Soundfont not loaded; call load_soundfont() first")
            }
        }
    }
}

impl std::error::Error for SoundfontError {}

/// Current state of the soundfont synthesizer
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SoundfontState {
    /// Not yet initialized
    #[default]
    Uninitialized,
    /// Currently loading soundfont
    Loading,
    /// Successfully loaded and ready for playback
    Ready,
    /// Failed to load; fallback to MP3
    Fallback,
    /// Explicitly disabled by user
    Disabled,
    /// Unloading in progress
    Unloading,
}

/// Configuration for soundfont playback
#[derive(Clone, Debug)]
pub struct SoundfontConfig {
    /// Path to the soundfont file (.sf2)
    pub path: String,
    /// Sample rate for synthesis (default: 44100 Hz)
    pub sample_rate: u32,
    /// Number of simultaneous voices (polyphony, default: 256)
    pub voices: u32,
    /// Enable reverb effect (default: true)
    pub reverb: bool,
    /// Enable chorus effect (default: false)
    pub chorus: bool,
    /// Master volume 0.0 - 1.0 (default: 0.8)
    pub volume: f32,
    /// MIDI channel to use (default: 0)
    pub channel: u8,
}

impl Default for SoundfontConfig {
    fn default() -> Self {
        Self {
            path: "assets/sounds/sf2/piano.sf2".to_string(),
            sample_rate: 44100,
            voices: 256,
            reverb: true,
            chorus: false,
            volume: 0.8,
            channel: 0,
        }
    }
}

impl SoundfontConfig {
    /// Create a new config with a custom path
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            ..Default::default()
        }
    }

    /// Set the sample rate
    pub fn with_sample_rate(mut self, rate: u32) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Set the polyphony (voice count)
    pub fn with_voices(mut self, voices: u32) -> Self {
        self.voices = voices;
        self
    }

    /// Set reverb enabled
    pub fn with_reverb(mut self, enabled: bool) -> Self {
        self.reverb = enabled;
        self
    }

    /// Set chorus enabled
    pub fn with_chorus(mut self, enabled: bool) -> Self {
        self.chorus = enabled;
        self
    }

    /// Set master volume
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume.clamp(0.0, 1.0);
        self
    }
}

/// Core soundfont synthesizer trait.
///
/// This trait defines the interface for soundfont synthesis operations.
/// Only Send is required (not Sync) since we manage internal synchronization
/// via Arc<RwLock<...>> in the FluidLiteSynth implementation.
pub trait SoundfontSynth: Send {
    /// Play a MIDI note with specified velocity
    ///
    /// # Arguments
    /// * `channel` - MIDI channel (0-15)
    /// * `note` - MIDI note number (0-127, typically 21-108 for piano)
    /// * `velocity` - Note velocity/strength (0-127)
    fn note_on(&self, channel: u8, note: u8, velocity: u8);

    /// Release a MIDI note (note off)
    ///
    /// # Arguments
    /// * `channel` - MIDI channel (0-15)
    /// * `note` - MIDI note number (0-127)
    fn note_off(&self, channel: u8, note: u8);

    /// Stop all currently playing notes
    fn all_notes_off(&self);

    /// Check if the synthesizer is ready for playback
    fn is_ready(&self) -> bool;

    /// Get current synthesizer state
    fn state(&self) -> SoundfontState;

    /// Get configuration info for diagnostics
    fn config(&self) -> Option<SoundfontConfig>;

    /// Reset the synthesizer to initial state
    fn reset(&self);

    /// Try to extract the inner FluidLiteSynth for use with rodio Source.
    /// Returns None if the implementation doesn't support this or if already taken.
    #[cfg(feature = "soundfont")]
    fn try_into_fluidlite(self: Box<Self>) -> Option<FluidLiteSynth>;
}

// =============================================================================
// FluidLite Implementation
// =============================================================================

#[cfg(feature = "soundfont")]
mod fluidlite_impl {
    use super::*;
    use rodio::Source;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::Duration;

    /// FluidLite-based synthesizer implementation.
    ///
    /// This struct wraps the fluidlite C library bindings with a safe Rust interface.
    /// It manages the synthesizer settings, bank/preset selection, and MIDI event
    /// processing.
    ///
    /// Note: This struct manages its own internal synchronization. The fluidlite::Synth
    /// is wrapped in a std::sync::Mutex to provide thread-safe access while maintaining
    /// the Send requirement for the SoundfontSynth trait.
    pub struct FluidLiteSynth {
        // Fluidlite synthesizer wrapped in Mutex for thread-safe access
        // Mutex ensures only one thread accesses the synth at a time
        synth: std::sync::Mutex<Option<fluidlite::Synth>>,

        // Configuration
        config: std::sync::Mutex<SoundfontConfig>,

        // State management
        state: std::sync::atomic::AtomicU32, // SoundfontState as u32 for atomic ops
        loaded: AtomicBool,

        // Performance metrics
        voice_count: AtomicU32,
        sample_rate: u32,
    }

    // SoundfontState to/from u32 for atomic operations
    const STATE_UNINITIALIZED: u32 = 0;
    const STATE_LOADING: u32 = 1;
    const STATE_READY: u32 = 2;
    const STATE_FALLBACK: u32 = 3;
    const STATE_DISABLED: u32 = 4;
    const STATE_UNLOADING: u32 = 5;

    impl SoundfontState {
        #[allow(dead_code)]
        fn as_u32(&self) -> u32 {
            match self {
                SoundfontState::Uninitialized => STATE_UNINITIALIZED,
                SoundfontState::Loading => STATE_LOADING,
                SoundfontState::Ready => STATE_READY,
                SoundfontState::Fallback => STATE_FALLBACK,
                SoundfontState::Disabled => STATE_DISABLED,
                SoundfontState::Unloading => STATE_UNLOADING,
            }
        }

        fn from_u32(val: u32) -> Self {
            match val {
                STATE_LOADING => SoundfontState::Loading,
                STATE_READY => SoundfontState::Ready,
                STATE_FALLBACK => SoundfontState::Fallback,
                STATE_DISABLED => SoundfontState::Disabled,
                STATE_UNLOADING => SoundfontState::Unloading,
                _ => SoundfontState::Uninitialized,
            }
        }
    }

    impl FluidLiteSynth {
        /// Create a new FluidLite synthesizer instance
        pub fn new() -> SoundfontResult<Self> {
            // Create settings
            let settings = fluidlite::Settings::new().map_err(|e| {
                SoundfontError::SynthInitError(format!("Settings creation failed: {}", e))
            })?;

            // Create synthesizer
            let synth = fluidlite::Synth::new(settings).map_err(|e| {
                SoundfontError::SynthInitError(format!("Synthesizer creation failed: {}", e))
            })?;

            Ok(Self {
                synth: std::sync::Mutex::new(Some(synth)),
                config: std::sync::Mutex::new(SoundfontConfig::default()),
                state: std::sync::atomic::AtomicU32::new(STATE_UNINITIALIZED),
                loaded: AtomicBool::new(false),
                voice_count: AtomicU32::new(0),
                sample_rate: 44100,
            })
        }

        /// Load a soundfont from file
        pub fn load_soundfont(&mut self, config: SoundfontConfig) -> SoundfontResult<()> {
            // Check state
            if self.loaded.load(Ordering::SeqCst) {
                return Err(SoundfontError::AlreadyLoaded);
            }

            self.state.store(STATE_LOADING, Ordering::SeqCst);

            // Validate file exists
            let path = Path::new(&config.path);
            if !path.exists() {
                self.state.store(STATE_FALLBACK, Ordering::SeqCst);
                return Err(SoundfontError::FileNotFound(config.path.clone()));
            }

            // Get write access to synthesizer
            let mut synth_guard = self.synth.lock().unwrap();
            let synth = synth_guard.as_mut().ok_or_else(|| {
                SoundfontError::SynthInitError("Synthesizer not initialized".to_string())
            })?;

            // Set sample rate on the synth (not via settings)
            synth.set_sample_rate(config.sample_rate as f32);

            // Set polyphony
            synth.set_polyphony(config.voices).map_err(|e| {
                SoundfontError::SynthInitError(format!("Polyphony setting failed: {}", e))
            })?;

            // Load the soundfont
            // fluidlite uses an ID system; first font is ID 0
            let font_id = synth.sfload(&config.path, true).map_err(|e| {
                self.state.store(STATE_FALLBACK, Ordering::SeqCst);
                SoundfontError::ParseError(format!(
                    "Failed to load soundfont: {} (path: {})",
                    e, config.path
                ))
            })?;

            log::info!("Loaded soundfont '{}' as font ID {}", config.path, font_id);

            // Select default program (preset 0 on bank 0) on channel 0
            #[allow(unused_must_use)]
            synth.program_select(0, font_id, 0, 0);

            // Configure reverb - use set_reverb_params method
            if config.reverb {
                // set_reverb_params takes: roomsize, damp, width, level (all f64)
                synth.set_reverb_params(0.2, 0.5, 0.8, 0.5);
                synth.set_reverb_on(true);
            } else {
                synth.set_reverb_on(false);
            }

            // Configure chorus - use set_chorus_params method
            // Note: fluidlite 0.2.1 set_chorus_params signature:
            // fn set_chorus_params(&self, nr: u32, level: f64, speed: f64, depth: f64, mode: /* some enum */)
            // Since ChorusMode may not be in public API, we use set_chorus_on/off instead
            if config.chorus {
                synth.set_chorus_on(true);
            } else {
                synth.set_chorus_on(false);
            }

            // Set master gain (0.0 to 1.0)
            let gain = config.volume;
            synth.set_gain(gain);

            // Store configuration
            *self.config.lock().unwrap() = config.clone();
            self.loaded.store(true, Ordering::SeqCst);
            self.state.store(STATE_READY, Ordering::SeqCst);
            self.sample_rate = config.sample_rate;

            log::info!(
                "FluidLite synthesizer ready: {} Hz, {} voices",
                config.sample_rate,
                config.voices
            );

            Ok(())
        }

        /// Get the raw synthesizer for integration with rodio
        pub fn get_synth(&self) -> std::sync::MutexGuard<'_, Option<fluidlite::Synth>> {
            self.synth.lock().unwrap()
        }

        /// Process a block of audio samples
        /// Returns the number of stereo samples written to the buffer
        pub fn process(&self, buffer: &mut [f32], frames: usize) {
            let synth_guard = self.synth.lock().unwrap();
            if let Some(ref synth) = *synth_guard {
                let stereo_frames = frames.min(buffer.len() / 2);
                // fluidlite write method expects a buffer that implements IsSamples trait
                // For f32 buffer, we use write_f32 with interleaved L/R pointers
                #[allow(unused_must_use)]
                unsafe {
                    synth.write_f32(
                        stereo_frames,
                        buffer.as_mut_ptr(),
                        0,
                        1,
                        buffer.as_mut_ptr().add(1),
                        0,
                        1,
                    );
                }
            }
        }
    }

    impl Default for FluidLiteSynth {
        fn default() -> Self {
            Self::new().expect("Failed to create FluidLiteSynth")
        }
    }

    impl Drop for FluidLiteSynth {
        fn drop(&mut self) {
            self.all_notes_off();
            self.state.store(STATE_UNLOADING, Ordering::SeqCst);
            log::info!("FluidLite synthesizer resources released");
        }
    }

    impl SoundfontSynth for FluidLiteSynth {
        fn note_on(&self, channel: u8, note: u8, velocity: u8) {
            if self.loaded.load(Ordering::SeqCst)
                && let Ok(synth_guard) = self.synth.lock()
                && let Some(ref synth) = *synth_guard
            {
                synth
                    .note_on(channel.into(), note.into(), velocity.into())
                    .ok();
                self.voice_count.fetch_add(1, Ordering::SeqCst);
                log::trace!("Note ON: ch={}, note={}, vel={}", channel, note, velocity);
            }
        }

        fn note_off(&self, channel: u8, note: u8) {
            if self.loaded.load(Ordering::SeqCst)
                && let Ok(synth_guard) = self.synth.lock()
                && let Some(ref synth) = *synth_guard
            {
                synth.note_off(channel.into(), note.into()).ok();
                self.voice_count.fetch_sub(1, Ordering::SeqCst);
                log::trace!("Note OFF: ch={}, note={}", channel, note);
            }
        }

        fn all_notes_off(&self) {
            if let Ok(synth_guard) = self.synth.lock()
                && let Some(ref synth) = *synth_guard
            {
                #[allow(unused_must_use)]
                synth.system_reset();
                self.voice_count.store(0, Ordering::SeqCst);
                log::debug!("All notes off");
            }
        }

        fn is_ready(&self) -> bool {
            self.loaded.load(Ordering::SeqCst) && self.state.load(Ordering::SeqCst) == STATE_READY
        }

        fn state(&self) -> SoundfontState {
            SoundfontState::from_u32(self.state.load(Ordering::SeqCst))
        }

        fn config(&self) -> Option<SoundfontConfig> {
            self.config.lock().ok().map(|c| c.clone())
        }

        fn reset(&self) {
            if let Ok(synth_guard) = self.synth.lock()
                && let Some(ref synth) = *synth_guard
            {
                #[allow(unused_must_use)]
                synth.system_reset();
                self.voice_count.store(0, Ordering::SeqCst);
                log::debug!("Synthesizer reset");
            }
        }

        #[cfg(feature = "soundfont")]
        fn try_into_fluidlite(self: Box<Self>) -> Option<FluidLiteSynth> {
            // We need to extract the inner synth. This requires accessing the internal fields.
            // Since we can't move out of Box<Self>, we use a workaround.
            // This is a workaround - in practice FluidLiteSynth is not dynSafe for this operation.
            // We'll handle this differently in audio_manager.rs by storing FluidLiteSynth directly.
            None
        }
    }

    /// Rodio Source wrapper for FluidLiteSynth.
    ///
    /// This struct wraps a FluidLiteSynth and implements rodio's Source trait
    /// so it can be mixed with other audio sources in the rodio pipeline.
    pub struct SoundfontSource {
        /// The synthesizer wrapped in an Arc for shared ownership
        synth: Arc<std::sync::Mutex<Option<fluidlite::Synth>>>,
        /// Buffer for generating interleaved stereo samples
        buffer: Vec<f32>,
        /// Current position in the buffer
        pos: usize,
        /// Number of frames to generate per buffer refill
        frames_per_fill: usize,
        /// Sample rate
        sample_rate: u32,
        /// Number of channels (always 2 for stereo)
        channels: u16,
    }

    impl SoundfontSource {
        /// Create a new SoundfontSource from a FluidLiteSynth.
        /// This takes ownership of the inner fluidlite synth from the FluidLiteSynth.
        pub fn from_fluidlite(synth: FluidLiteSynth) -> Self {
            let sample_rate = synth.sample_rate;
            let frames_per_fill = 512;
            let buffer_size = frames_per_fill * 2; // stereo

            // Take the inner synth from FluidLiteSynth
            let inner_synth = synth.synth.lock().unwrap().take();

            Self {
                synth: Arc::new(std::sync::Mutex::new(inner_synth)),
                buffer: vec![0.0; buffer_size],
                pos: buffer_size, // Start with empty buffer to trigger refill
                frames_per_fill,
                sample_rate,
                channels: 2,
            }
        }
    }

    impl From<FluidLiteSynth> for SoundfontSource {
        fn from(synth: FluidLiteSynth) -> Self {
            Self::from_fluidlite(synth)
        }
    }

    impl SoundfontSource {
        fn refill_buffer(&mut self) {
            let mut guard = self.synth.lock().unwrap();
            if let Some(ref mut synth) = *guard {
                let stereo_frames = self.frames_per_fill;
                self.buffer.resize(stereo_frames * 2, 0.0);

                #[allow(unused_must_use)]
                unsafe {
                    synth.write_f32(
                        stereo_frames,
                        self.buffer.as_mut_ptr(),
                        0,
                        1,
                        self.buffer.as_mut_ptr().add(1),
                        0,
                        1,
                    );
                }
                self.pos = 0;
            }
            drop(guard);
        }

        pub fn into_synth(mut self) -> Arc<std::sync::Mutex<Option<fluidlite::Synth>>> {
            let synth_arc = self.synth.clone();
            self.synth = Arc::new(std::sync::Mutex::new(None));
            synth_arc
        }

        /// Clone the inner synth Arc without consuming self.
        /// This allows both the AudioManager (for note control) and the mixer (for audio output)
        /// to share the same synthesizer.
        pub fn clone_synth_arc(&self) -> Arc<std::sync::Mutex<Option<fluidlite::Synth>>> {
            self.synth.clone()
        }

        pub fn sample_rate(&self) -> u32 {
            self.sample_rate
        }
    }

    impl Iterator for SoundfontSource {
        type Item = f32;

        #[inline]
        fn next(&mut self) -> Option<Self::Item> {
            // Refill buffer if empty
            if self.pos >= self.buffer.len() {
                self.refill_buffer();
            }

            if self.pos < self.buffer.len() {
                let sample = self.buffer[self.pos];
                self.pos += 1;
                Some(sample)
            } else {
                None
            }
        }

        #[inline]
        fn size_hint(&self) -> (usize, Option<usize>) {
            // Return infinite to keep the source playing
            (usize::MAX, None)
        }
    }

    impl Source for SoundfontSource {
        #[inline]
        fn current_frame_len(&self) -> Option<usize> {
            None // Infinite source
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
            None // Infinite duration
        }
    }

    /// Extract the inner synthesizer from this source for use with the SoundfontSynth trait.
    /// After calling this, the SoundfontSource is no longer usable for audio output.
    #[allow(dead_code)]
    pub fn into_synth_global(
        synth: SoundfontSource,
    ) -> Arc<std::sync::Mutex<Option<fluidlite::Synth>>> {
        synth.synth.clone()
    }

    /// A wrapper that implements SoundfontSynth by holding an Arc to the raw synthesizer.
    /// This allows the trait object to be stored in AudioManager while the SoundfontSource
    /// (which feeds audio into the rodio mixer) holds its own Arc clone.
    pub struct ArcSoundfontSynth {
        synth: Arc<std::sync::Mutex<Option<fluidlite::Synth>>>,
        loaded: AtomicBool,
    }

    impl ArcSoundfontSynth {
        pub fn new(synth: Arc<std::sync::Mutex<Option<fluidlite::Synth>>>) -> Self {
            let loaded = synth.lock().ok().is_some_and(|g| g.is_some());
            Self {
                synth,
                loaded: AtomicBool::new(loaded),
            }
        }
    }

    impl SoundfontSynth for ArcSoundfontSynth {
        fn note_on(&self, channel: u8, note: u8, velocity: u8) {
            if let Ok(guard) = self.synth.lock()
                && let Some(ref synth) = *guard
            {
                synth
                    .note_on(channel.into(), note.into(), velocity.into())
                    .ok();
            }
        }

        fn note_off(&self, channel: u8, note: u8) {
            if let Ok(guard) = self.synth.lock()
                && let Some(ref synth) = *guard
            {
                synth.note_off(channel.into(), note.into()).ok();
            }
        }

        fn all_notes_off(&self) {
            if let Ok(guard) = self.synth.lock()
                && let Some(ref synth) = *guard
            {
                #[allow(unused_must_use)]
                synth.system_reset();
            }
        }

        fn is_ready(&self) -> bool {
            self.loaded.load(Ordering::SeqCst)
        }

        fn state(&self) -> SoundfontState {
            if self.is_ready() {
                SoundfontState::Ready
            } else {
                SoundfontState::Uninitialized
            }
        }

        fn config(&self) -> Option<SoundfontConfig> {
            None
        }

        fn reset(&self) {
            self.all_notes_off();
        }

        #[cfg(feature = "soundfont")]
        fn try_into_fluidlite(self: Box<Self>) -> Option<FluidLiteSynth> {
            None
        }
    }
} // end mod fluidlite_impl

// =============================================================================
// Mock Implementation for non-soundfont builds
// =============================================================================

#[cfg(not(feature = "soundfont"))]
mod mock_synth {
    use super::*;

    /// Mock synthesizer for non-soundfont builds
    pub struct MockSynth {
        state: std::sync::atomic::AtomicU32,
    }

    impl MockSynth {
        pub fn new() -> Self {
            Self {
                state: std::sync::atomic::AtomicU32::new(STATE_UNINITIALIZED),
            }
        }
    }

    impl Default for MockSynth {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SoundfontSynth for MockSynth {
        fn note_on(&self, _channel: u8, _note: u8, _velocity: u8) {}
        fn note_off(&self, _channel: u8, _note: u8) {}
        fn all_notes_off(&self) {}
        fn is_ready(&self) -> bool {
            false
        }
        fn state(&self) -> SoundfontState {
            SoundfontState::Disabled
        }
        fn config(&self) -> Option<SoundfontConfig> {
            None
        }
        fn reset(&self) {}
    }
}

// =============================================================================
// Public Alias and Factory
// =============================================================================

#[cfg(feature = "soundfont")]
pub use fluidlite_impl::{ArcSoundfontSynth, FluidLiteSynth, SoundfontSource};

#[cfg(not(feature = "soundfont"))]
pub use mock_synth::MockSynth as FluidLiteSynth;

/// Create a new soundfont synthesizer instance
pub fn create_synthesizer() -> SoundfontResult<Box<dyn SoundfontSynth>> {
    #[cfg(feature = "soundfont")]
    {
        Ok(Box::new(FluidLiteSynth::new()?))
    }
    #[cfg(not(feature = "soundfont"))]
    {
        let _ = create_synthesizer; // silence unused warning
        Err(SoundfontError::NotEnabled)
    }
}

// =============================================================================
// Path Resolution Utilities
// =============================================================================

/// Resolve soundfont path with cross-platform support.
///
/// Resolution precedence:
/// 1. CLI-provided absolute path
/// 2. CLI-provided relative to executable directory
/// 3. Built-in default relative to executable directory
/// 4. Built-in default relative to current working directory
///
/// # Arguments
/// * `cli_path` - Optional path from CLI argument
/// * `exe_dir` - Directory of the executable
///
/// # Returns
/// Tuple of (resolved_path, source_type) where source_type indicates
/// where the path came from (for logging/display purposes)
pub fn resolve_soundfont_path(
    cli_path: Option<&str>,
    exe_dir: &Path,
) -> (std::path::PathBuf, &'static str) {
    // Priority 1: CLI absolute path
    if let Some(path) = cli_path {
        let p = std::path::PathBuf::from(path);
        if p.is_absolute() && p.exists() {
            log::info!("Using absolute soundfont path from CLI: {}", path);
            return (p, "cli_absolute");
        }

        // CLI relative path - check against executable directory
        let exe_relative = exe_dir.join(path);
        if exe_relative.exists() {
            log::info!(
                "Using soundfont path relative to executable: {:?}",
                exe_relative
            );
            return (exe_relative, "cli_relative_to_exe");
        }

        // CLI relative path - check against current directory
        let cwd_relative = std::path::PathBuf::from(path);
        if cwd_relative.exists() {
            log::info!("Using soundfont path relative to cwd: {:?}", cwd_relative);
            return (cwd_relative, "cli_relative_to_cwd");
        }

        // CLI path provided but not found - return anyway for error reporting
        log::warn!("CLI soundfont path not found: {}", path);
        return (p, "cli_not_found");
    }

    // Priority 2: Built-in default relative to executable
    let default_exe = exe_dir.join("assets/sounds/sf2/piano.sf2");
    if default_exe.exists() {
        log::info!("Using default soundfont path relative to executable");
        return (default_exe, "default_exe_relative");
    }

    // Priority 3: Built-in default relative to current working directory
    let default_cwd = std::path::PathBuf::from("assets/sounds/sf2/piano.sf2");
    if default_cwd.exists() {
        log::info!("Using default soundfont path relative to current directory");
        return (default_cwd, "default_cwd_relative");
    }

    // Nothing found - return default path for error reporting
    log::warn!("Default soundfont path not found; using built-in default");
    (default_exe, "default_not_found")
}

/// Validate that a soundfont file exists and can be read
pub fn validate_soundfont_path(path: &str) -> SoundfontResult<std::path::PathBuf> {
    let path_buf = std::path::PathBuf::from(path);

    if !path_buf.exists() {
        return Err(SoundfontError::FileNotFound(path.to_string()));
    }

    // Check if file is readable
    match std::fs::metadata(&path_buf) {
        Ok(meta) => {
            if meta.len() == 0 {
                return Err(SoundfontError::ParseError(format!(
                    "Soundfont file is empty: {}",
                    path
                )));
            }
            // Basic sanity check: SF2 files are typically > 1KB
            if meta.len() < 1024 {
                log::warn!("Soundfont file seems unusually small: {} bytes", meta.len());
            }
        }
        Err(e) => {
            return Err(SoundfontError::FileReadError(format!(
                "Cannot read soundfont metadata: {}",
                e
            )));
        }
    }

    Ok(path_buf)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soundfont_error_display() {
        let err = SoundfontError::FileNotFound("test.sf2".to_string());
        assert!(err.to_string().contains("test.sf2"));

        let err = SoundfontError::NotEnabled;
        assert!(err.to_string().contains("not compiled in"));
    }

    #[test]
    fn test_soundfont_config_default() {
        let config = SoundfontConfig::default();
        assert_eq!(config.sample_rate, 44100);
        assert_eq!(config.voices, 256);
        assert_eq!(config.volume, 0.8);
        assert!(config.reverb);
        assert!(!config.chorus);
    }

    #[test]
    fn test_soundfont_config_builder() {
        let config = SoundfontConfig::new("custom.sf2")
            .with_sample_rate(48000)
            .with_voices(512)
            .with_volume(0.5);

        assert_eq!(config.path, "custom.sf2");
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.voices, 512);
        assert_eq!(config.volume, 0.5);
    }

    #[test]
    fn test_volume_clamping() {
        let config = SoundfontConfig::new("test.sf2").with_volume(1.5);
        assert_eq!(config.volume, 1.0); // Clamped to 1.0

        let config = SoundfontConfig::new("test.sf2").with_volume(-0.5);
        assert_eq!(config.volume, 0.0); // Clamped to 0.0
    }

    #[test]
    fn test_soundfont_state_default() {
        let state = SoundfontState::default();
        assert_eq!(state, SoundfontState::Uninitialized);
    }

    #[cfg(feature = "soundfont")]
    #[test]
    fn test_fluidlite_synth_creation() {
        // This test only runs if soundfont feature is enabled
        let result = FluidLiteSynth::new();
        assert!(result.is_ok());

        let synth = result.unwrap();
        assert_eq!(synth.state(), SoundfontState::Uninitialized);
        assert!(!synth.is_ready());
    }

    #[test]
    fn test_path_resolution_empty_cli() {
        // Test with no CLI path - should fall through to defaults
        let exe_dir = Path::new("/fake/exe");
        let (path, source) = resolve_soundfont_path(None, exe_dir);

        assert!(path.to_string_lossy().contains("piano.sf2"));
        // Source will be "default_*" since no CLI path
        assert!(source.starts_with("default"));
    }

    #[test]
    fn test_path_resolution_with_nonexistent_cli() {
        let exe_dir = Path::new("/fake/exe");
        let (path, source) = resolve_soundfont_path(Some("/nonexistent.sf2"), exe_dir);

        assert!(path.to_string_lossy().ends_with("/nonexistent.sf2"));
        assert_eq!(source, "cli_not_found");
    }

    #[test]
    fn test_validate_soundfont_empty_path() {
        // Empty path should fail
        let result = validate_soundfont_path("");
        assert!(result.is_err());

        match result {
            Err(SoundfontError::FileNotFound(_)) => {}
            other => panic!("Expected FileNotFound error, got: {:?}", other),
        }
    }
}
