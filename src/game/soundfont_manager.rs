//! Soundfont synthesis management module.
//!
//! This module provides safe Rust bindings to TinySoundFont for software synthesizer
//! playback using SoundFont (.sf2) files. It integrates with the existing rodio
//! audio pipeline via the DynamicMixer architecture.
//!
//! # Features
//! - Lazy loading of soundfont files
//! - MIDI note playback (note_on/note_off)
//! - Configurable polyphony
//! - Thread-safe access from game loop
//! - Automatic fallback support
//!
//! # Example
//! ```ignore
//! let config = SoundfontConfig::default();
//! let mut synth = TinySFSynth::new();
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
            volume: 0.25,
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
/// via Arc<Mutex<...>> in the TinySFSynth implementation.
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
}

// =============================================================================
// TinySoundFont Implementation
// =============================================================================

/// Shared handle to a TinySoundFont synthesizer, wrapped for thread-safe access.
/// The inner `Option<*mut tsf>` is Some when a soundfont is loaded, None after unload.
#[cfg(feature = "soundfont")]
pub type SharedTsfHandle = std::sync::Arc<std::sync::Mutex<Option<*mut crate::tsf_bindings::tsf>>>;

#[cfg(feature = "soundfont")]
mod tsf_impl {
    use super::*;
    use crate::tsf_bindings::{
        self, TSFOutputMode, tsf, tsf_channel_note_off, tsf_channel_note_on,
        tsf_channel_set_presetnumber, tsf_close, tsf_note_off_all, tsf_render_float, tsf_reset,
        tsf_set_max_voices, tsf_set_output, tsf_set_volume,
    };
    use rodio::Source;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::Duration;

    /// TinySoundFont-based synthesizer implementation.
    ///
    /// This struct wraps the TinySoundFont C library with a safe Rust interface.
    /// It manages the synthesizer settings, preset selection, and MIDI event
    /// processing.
    ///
    /// The opaque `*mut tsf` handle is wrapped in a std::sync::Mutex to provide
    /// thread-safe access while maintaining the Send requirement for the
    /// SoundfontSynth trait.
    pub struct TinySFSynth {
        /// TinySoundFont synthesizer handle (nullable, owned)
        synth: std::sync::Mutex<Option<*mut tsf>>,

        /// Configuration
        config: std::sync::Mutex<SoundfontConfig>,

        /// State management
        state: AtomicU32, // SoundfontState as u32 for atomic ops
        loaded: AtomicBool,

        /// Performance metrics
        voice_count: AtomicU32,
        sample_rate: u32,

        /// Current preset index on the MIDI channel
        preset_index: AtomicU32,
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

    // Safety: TinySFSynth uses Mutex for interior mutability and the tsf handle
    // is only accessed through the mutex. The tsf handle itself is Send because
    // it's a raw pointer to a C struct that we manage exclusively.
    unsafe impl Send for TinySFSynth {}

    impl TinySFSynth {
        /// Create a new TinySoundFont synthesizer instance.
        /// The synthesizer is initially empty; call `load_soundfont()` to load a .sf2 file.
        pub fn new() -> SoundfontResult<Self> {
            Ok(Self {
                synth: std::sync::Mutex::new(None),
                config: std::sync::Mutex::new(SoundfontConfig::default()),
                state: AtomicU32::new(STATE_UNINITIALIZED),
                loaded: AtomicBool::new(false),
                voice_count: AtomicU32::new(0),
                sample_rate: 44100,
                preset_index: AtomicU32::new(u32::MAX),
            })
        }

        /// Load a soundfont from file.
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

            // Read the .sf2 file into memory (tsf_load_memory requires a contiguous buffer)
            let sf2_data = std::fs::read(&config.path).map_err(|e| {
                self.state.store(STATE_FALLBACK, Ordering::SeqCst);
                SoundfontError::FileReadError(format!(
                    "Failed to read soundfont '{}': {}",
                    config.path, e
                ))
            })?;

            if sf2_data.is_empty() {
                self.state.store(STATE_FALLBACK, Ordering::SeqCst);
                return Err(SoundfontError::ParseError(format!(
                    "Soundfont file is empty: {}",
                    config.path
                )));
            }

            // Load via TinySoundFont
            let tsf_handle = unsafe {
                tsf_bindings::tsf_load_memory(
                    sf2_data.as_ptr() as *const std::ffi::c_void,
                    sf2_data.len() as std::os::raw::c_int,
                )
            };

            if tsf_handle.is_null() {
                self.state.store(STATE_FALLBACK, Ordering::SeqCst);
                return Err(SoundfontError::ParseError(format!(
                    "Failed to parse soundfont: {}",
                    config.path
                )));
            }

            // Set output mode: stereo interleaved, at the configured sample rate
            // global_gain_db: 0.0 means no dB adjustment; we control volume via tsf_set_volume
            unsafe {
                tsf_set_output(
                    tsf_handle,
                    TSFOutputMode::TSF_STEREO_INTERLEAVED,
                    config.sample_rate as std::os::raw::c_int,
                    0.0,
                );
            }

            // Set polyphony (max voices)
            unsafe {
                let result = tsf_set_max_voices(tsf_handle, config.voices as std::os::raw::c_int);
                if result == 0 {
                    // Allocation failed — not critical, synth will use default
                    log::warn!(
                        "Failed to set max voices to {}; using default",
                        config.voices
                    );
                }
            }

            // Set channel preset: use preset 0 (first preset) on channel 0
            let preset_idx = unsafe { tsf_bindings::tsf_get_presetindex(tsf_handle, 0, 0) };
            if preset_idx < 0 {
                // No preset found at bank 0, preset 0 — try first available
                let count = unsafe { tsf_bindings::tsf_get_presetcount(tsf_handle) };
                if count <= 0 {
                    unsafe {
                        tsf_close(tsf_handle);
                    }
                    self.state.store(STATE_FALLBACK, Ordering::SeqCst);
                    return Err(SoundfontError::NoPresets(config.path.clone()));
                }
                // Use the first preset
                unsafe {
                    tsf_channel_set_presetnumber(
                        tsf_handle,
                        config.channel as std::os::raw::c_int,
                        0,
                        0,
                    );
                }
                self.preset_index.store(0, Ordering::SeqCst);
            } else {
                unsafe {
                    tsf_channel_set_presetnumber(
                        tsf_handle,
                        config.channel as std::os::raw::c_int,
                        0,
                        0,
                    );
                }
                self.preset_index.store(preset_idx as u32, Ordering::SeqCst);
            }

            // Set master volume (linear scale)
            unsafe {
                tsf_set_volume(tsf_handle, config.volume);
            }

            // Store the handle
            {
                let mut synth_guard = self.synth.lock().unwrap();
                *synth_guard = Some(tsf_handle);
            }

            // Store configuration
            *self.config.lock().unwrap() = config.clone();
            self.loaded.store(true, Ordering::SeqCst);
            self.state.store(STATE_READY, Ordering::SeqCst);
            self.sample_rate = config.sample_rate;

            log::info!(
                "TinySoundFont synthesizer ready: {} Hz, {} voices",
                config.sample_rate,
                config.voices
            );

            Ok(())
        }

        /// Get the raw tsf handle for integration with rodio.
        /// Returns a MutexGuard wrapping the optional raw pointer.
        pub fn get_synth(&self) -> std::sync::MutexGuard<'_, Option<*mut tsf>> {
            self.synth.lock().unwrap()
        }

        /// Process a block of audio samples using tsf_render_float.
        /// The buffer is interpreted as interleaved stereo: [L, R, L, R, ...]
        /// `frames` is the number of stereo frames to render.
        pub fn process(&self, buffer: &mut [f32], frames: usize) {
            let synth_guard = self.synth.lock().unwrap();
            if let Some(handle) = *synth_guard {
                let stereo_frames = frames.min(buffer.len() / 2);
                unsafe {
                    tsf_render_float(
                        handle,
                        buffer.as_mut_ptr(),
                        stereo_frames as std::os::raw::c_int,
                        0, // flag_mixing: 0 = clear buffer first
                    );
                }
            }
        }
    }

    impl Default for TinySFSynth {
        fn default() -> Self {
            Self::new().expect("Failed to create TinySFSynth")
        }
    }

    impl Drop for TinySFSynth {
        fn drop(&mut self) {
            self.all_notes_off();
            self.state.store(STATE_UNLOADING, Ordering::SeqCst);
            // Free the tsf handle
            let mut synth_guard = self.synth.lock().unwrap();
            if let Some(handle) = synth_guard.take() {
                unsafe {
                    tsf_close(handle);
                }
            }
            log::info!("TinySoundFont synthesizer resources released");
        }
    }

    impl SoundfontSynth for TinySFSynth {
        fn note_on(&self, channel: u8, note: u8, velocity: u8) {
            if self.loaded.load(Ordering::SeqCst) {
                let synth_guard = self.synth.lock().unwrap();
                if let Some(handle) = *synth_guard {
                    // Convert velocity from 0-127 to 0.0-1.0 float range
                    let vel_float = velocity as f32 / 127.0;
                    let result = unsafe {
                        tsf_channel_note_on(
                            handle,
                            channel as std::os::raw::c_int,
                            note as std::os::raw::c_int,
                            vel_float,
                        )
                    };
                    if result != 0 {
                        self.voice_count.fetch_add(1, Ordering::SeqCst);
                    }
                    log::trace!("Note ON: ch={}, note={}, vel={}", channel, note, velocity);
                }
            }
        }

        fn note_off(&self, channel: u8, note: u8) {
            if self.loaded.load(Ordering::SeqCst) {
                let synth_guard = self.synth.lock().unwrap();
                if let Some(handle) = *synth_guard {
                    unsafe {
                        tsf_channel_note_off(
                            handle,
                            channel as std::os::raw::c_int,
                            note as std::os::raw::c_int,
                        );
                    }
                    self.voice_count.fetch_sub(1, Ordering::SeqCst);
                    log::trace!("Note OFF: ch={}, note={}", channel, note);
                }
            }
        }

        fn all_notes_off(&self) {
            let synth_guard = self.synth.lock().unwrap();
            if let Some(handle) = *synth_guard {
                unsafe {
                    tsf_note_off_all(handle);
                }
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
            let synth_guard = self.synth.lock().unwrap();
            if let Some(handle) = *synth_guard {
                unsafe {
                    tsf_reset(handle);
                }
                self.voice_count.store(0, Ordering::SeqCst);
                log::debug!("Synthesizer reset");
            }
        }
    }

    /// Rodio Source wrapper for TinySFSynth.
    ///
    /// This struct wraps a TinySFSynth and implements rodio's Source trait
    /// so it can be mixed with other audio sources in the rodio pipeline.
    pub struct SoundfontSource {
        /// The synthesizer handle (raw pointer, shared ownership via Arc)
        synth: Arc<std::sync::Mutex<Option<*mut tsf>>>,
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

    // Safety: SoundfontSource uses Arc<Mutex<*mut tsf>> which is Send + Sync
    unsafe impl Send for SoundfontSource {}
    unsafe impl Sync for SoundfontSource {}

    impl SoundfontSource {
        /// Create a new SoundfontSource from a TinySFSynth.
        /// This takes the inner tsf handle from the TinySFSynth.
        pub fn from_tinysf(synth: TinySFSynth) -> Self {
            let sample_rate = synth.sample_rate;
            let frames_per_fill = 512;
            let buffer_size = frames_per_fill * 2; // stereo

            // Take the inner tsf handle from TinySFSynth
            let inner_handle = synth.synth.lock().unwrap().take();

            Self {
                synth: Arc::new(std::sync::Mutex::new(inner_handle)),
                buffer: vec![0.0; buffer_size],
                pos: buffer_size, // Start with empty buffer to trigger refill
                frames_per_fill,
                sample_rate,
                channels: 2,
            }
        }
    }

    impl From<TinySFSynth> for SoundfontSource {
        fn from(synth: TinySFSynth) -> Self {
            Self::from_tinysf(synth)
        }
    }

    impl SoundfontSource {
        /// Create a new SoundfontSource from a shared synth Arc and sample rate.
        /// This is used to recreate the source after a mixer rebuild.
        pub fn from_synth_arc(
            synth: Arc<std::sync::Mutex<Option<*mut tsf>>>,
            sample_rate: u32,
        ) -> Self {
            let frames_per_fill = 512;
            let buffer_size = frames_per_fill * 2; // stereo
            Self {
                synth,
                buffer: vec![0.0; buffer_size],
                pos: buffer_size, // Start with empty buffer to trigger refill
                frames_per_fill,
                sample_rate,
                channels: 2,
            }
        }

        fn refill_buffer(&mut self) {
            let guard = self.synth.lock().unwrap();
            if let Some(handle) = *guard {
                let stereo_frames = self.frames_per_fill;
                self.buffer.resize(stereo_frames * 2, 0.0);

                unsafe {
                    tsf_render_float(
                        handle,
                        self.buffer.as_mut_ptr(),
                        stereo_frames as std::os::raw::c_int,
                        0, // flag_mixing: 0 = clear buffer first
                    );
                }
                self.pos = 0;
            }
            drop(guard);
        }

        pub fn into_synth(mut self) -> Arc<std::sync::Mutex<Option<*mut tsf>>> {
            let synth_arc = self.synth.clone();
            self.synth = Arc::new(std::sync::Mutex::new(None));
            synth_arc
        }

        /// Clone the inner synth Arc without consuming self.
        pub fn clone_synth_arc(&self) -> Arc<std::sync::Mutex<Option<*mut tsf>>> {
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

    /// Extract the inner synthesizer Arc from a SoundfontSource.
    /// After calling this, the SoundfontSource is no longer usable for audio output.
    #[allow(dead_code)]
    pub fn into_synth_global(synth: SoundfontSource) -> Arc<std::sync::Mutex<Option<*mut tsf>>> {
        synth.synth.clone()
    }

    /// A wrapper that implements SoundfontSynth by holding an Arc to the raw tsf handle.
    /// This allows the trait object to be stored in AudioManager while the SoundfontSource
    /// (which feeds audio into the rodio mixer) holds its own Arc clone.
    pub struct ArcSoundfontSynth {
        synth: Arc<std::sync::Mutex<Option<*mut tsf>>>,
        loaded: AtomicBool,
    }

    unsafe impl Send for ArcSoundfontSynth {}
    unsafe impl Sync for ArcSoundfontSynth {}

    impl ArcSoundfontSynth {
        pub fn new(synth: Arc<std::sync::Mutex<Option<*mut tsf>>>) -> Self {
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
                && let Some(handle) = *guard
            {
                let vel_float = velocity as f32 / 127.0;
                unsafe {
                    tsf_channel_note_on(
                        handle,
                        channel as std::os::raw::c_int,
                        note as std::os::raw::c_int,
                        vel_float,
                    );
                }
            }
        }

        fn note_off(&self, channel: u8, note: u8) {
            if let Ok(guard) = self.synth.lock()
                && let Some(handle) = *guard
            {
                unsafe {
                    tsf_channel_note_off(
                        handle,
                        channel as std::os::raw::c_int,
                        note as std::os::raw::c_int,
                    );
                }
            }
        }

        fn all_notes_off(&self) {
            if let Ok(guard) = self.synth.lock()
                && let Some(handle) = *guard
            {
                unsafe {
                    tsf_note_off_all(handle);
                }
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
    }
} // end mod tsf_impl

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
                state: std::sync::atomic::AtomicU32::new(0),
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
// SharedTsfHandle is already defined above as pub type
#[cfg(not(feature = "soundfont"))]
pub use mock_synth::MockSynth as TinySFSynth;
#[cfg(feature = "soundfont")]
pub use tsf_impl::{ArcSoundfontSynth, SoundfontSource, TinySFSynth};

/// Create a new soundfont synthesizer instance
pub fn create_synthesizer() -> SoundfontResult<Box<dyn SoundfontSynth>> {
    #[cfg(feature = "soundfont")]
    {
        Ok(Box::new(TinySFSynth::new()?))
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