use super::midi_types::{MidiJson, midi_to_note, ticks_to_seconds};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[cfg(feature = "audio")]
use std::sync::{Arc, Mutex};

// Soundfont imports - conditionally compiled
#[cfg(feature = "soundfont")]
use super::soundfont_manager::{
    ArcSoundfontSynth, SendSyncTsfHandle, SoundfontConfig, SoundfontError, SoundfontResult,
    SoundfontState, SoundfontSynth, TinySFSynth, validate_soundfont_path,
};

// =============================================================================
// Audio backend types (feature-gated)
// =============================================================================

/// Internal representation of a pre-decoded audio sample.
/// All samples are stored as interleaved stereo f32 at 44100 Hz.
#[cfg(feature = "audio")]
struct DecodedSample {
    data: Arc<[f32]>,
}

/// A sample currently being played back, tracked by the audio callback.
#[cfg(feature = "audio")]
struct PlayingSample {
    /// Interleaved stereo f32 sample data (44100 Hz).
    data: Arc<[f32]>,
    /// Current playback position in samples (not frames).
    pos: usize,
}

/// Shared state between the main thread and the miniaudio data callback.
///
/// All mutable fields are guarded by `std::sync::Mutex` so they can be
/// safely accessed from both the audio thread (callback) and the main
/// thread (game loop).
#[cfg(feature = "audio")]
struct AudioCallbackData {
    /// Queue of currently-playing samples, mixed in the audio callback.
    playing: Mutex<Vec<PlayingSample>>,
    /// Reusable scratch buffer for soundfont rendering.
    synth_buffer: Mutex<Vec<f32>>,
    /// The soundfont synthesizer, accessed from the callback for rendering.
    #[cfg(feature = "soundfont")]
    synth: Mutex<Option<Arc<dyn SoundfontSynth + Send + Sync>>>,
}

/// RAII wrapper around a `ma_device`. When dropped, the device is stopped
/// (if running), then uninitialized. The `_callback_data` field keeps the
/// shared callback state alive for the entire lifetime of the device.
///
/// On Windows, `ma_device_init()` for the WASAPI backend calls
/// `CoInitializeEx(COINIT_MULTITHREADED)`, which conflicts with winit's
/// `OleInitialize()` (which needs `COINIT_APARTMENTTHREADED`). Therefore,
/// device initialization is deferred from `AudioManager::new()` to
/// `initialize_samples()`, which runs inside winit's `resumed()` handler —
/// after the window (and COM apartment) are already set up.
#[cfg(feature = "audio")]
struct AudioDevice {
    device: crate::miniaudio_bindings::ma_device,
    _callback_data: Arc<AudioCallbackData>,
    started: bool,
}

#[cfg(feature = "audio")]
impl AudioDevice {
    /// Start the device if it hasn't been started yet.
    /// Returns true if the device is now running, false if it failed to start.
    fn ensure_started(&mut self) -> bool {
        if self.started {
            return true;
        }
        use crate::miniaudio_bindings::{
            ma_device_start, ma_result_MA_SUCCESS,
        };
        // SAFETY: The device was previously initialized via ma_device_init.
        unsafe {
            let result = ma_device_start(&mut self.device);
            if result == ma_result_MA_SUCCESS {
                self.started = true;
                log::info!("Miniaudio playback device started (lazy)");
                true
            } else {
                log::error!("Failed to start miniaudio device (result={})", result);
                false
            }
        }
    }

    /// Check whether the device has been successfully started.
    fn is_started(&self) -> bool {
        self.started
    }
}

#[cfg(feature = "audio")]
impl Drop for AudioDevice {
    fn drop(&mut self) {
        // SAFETY: The device was previously initialized via ma_device_init.
        // If it was started, we stop it first. ma_device_stop blocks until
        // the data callback has finished, so no callback is running when we
        // call ma_device_uninit. The callback data Arc is still valid because
        // _callback_data is only dropped after _device is uninitialized.
        //
        // We use std::panic::catch_unwind to guard against assertion failures
        // in miniaudio during abnormal process teardown (e.g. when a panic in
        // winit triggers unwinding while the audio device is in an unexpected
        // state).
        let stop_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            unsafe {
                if self.started {
                    crate::miniaudio_bindings::ma_device_stop(&mut self.device);
                }
                crate::miniaudio_bindings::ma_device_uninit(&mut self.device);
            }
        }));
        if let Err(_) = stop_result {
            log::warn!(
                "AudioDevice::drop: caught panic during device cleanup \
                 (this can happen during process teardown after an earlier error)"
            );
        }
    }
}

// =============================================================================
// Audio callback and helper functions
// =============================================================================

/// Decode an MP3 file to interleaved stereo f32 at 44100 Hz using miniaudio.
///
/// Returns `Some(Arc<[f32]>)` on success, `None` on failure.
#[cfg(feature = "audio")]
fn decode_mp3_file(path: &std::path::Path) -> Option<Arc<[f32]>> {
    use crate::miniaudio_bindings::{
        ma_decoder, ma_decoder_config_init, ma_decoder_get_length_in_pcm_frames,
        ma_decoder_init_file, ma_decoder_read_pcm_frames, ma_decoder_uninit,
        ma_format_ma_format_f32, ma_result_MA_SUCCESS, ma_uint64,
    };

    let path_cstr = std::ffi::CString::new(path.to_str()?).ok()?;

    // SAFETY: We provide valid pointers. ma_decoder is stack-allocated and
    // will be fully initialized by ma_decoder_init_file on success.
    unsafe {
        let decoder_config = ma_decoder_config_init(ma_format_ma_format_f32, 2, 44100);

        let mut decoder = std::mem::MaybeUninit::<ma_decoder>::uninit();

        let result = ma_decoder_init_file(
            path_cstr.as_ptr(),
            &decoder_config,
            decoder.as_mut_ptr(),
        );

        if result != ma_result_MA_SUCCESS {
            log::warn!("Failed to init ma_decoder for {:?}: {}", path, result);
            return None;
        }

        let mut decoder = decoder.assume_init();

        // Query the total number of frames in the file.
        let mut total_frames: ma_uint64 = 0;
        ma_decoder_get_length_in_pcm_frames(&mut decoder, &mut total_frames);

        // For formats where the length is unknown, allocate a reasonable buffer.
        let buffer_frames = if total_frames > 0 {
            total_frames as usize
        } else {
            // Max 60 seconds of stereo audio as fallback.
            44100 * 60
        };

        let mut buffer = vec![0.0f32; buffer_frames * 2];

        let mut total_read: usize = 0;
        loop {
            let remaining = (buffer_frames - total_read) as ma_uint64;
            let mut frames_read: ma_uint64 = 0;
            let result = ma_decoder_read_pcm_frames(
                &mut decoder,
                buffer.as_mut_ptr().wrapping_add(total_read * 2) as *mut std::ffi::c_void,
                remaining,
                &mut frames_read,
            );

            if result != ma_result_MA_SUCCESS || frames_read == 0 {
                break;
            }
            total_read += frames_read as usize;
            if total_read >= buffer_frames {
                break;
            }
        }

        ma_decoder_uninit(&mut decoder);

        if total_read == 0 {
            log::warn!("No frames decoded from {:?}", path);
            return None;
        }

        buffer.truncate(total_read * 2);
        Some(Arc::from(buffer))
    }
}

/// The miniaudio data callback. Called from the audio thread for every
/// device period to fill the output buffer.
///
/// # Safety
///
/// - `p_device` must point to a valid, initialized `ma_device` whose
///   `pUserData` field points to an `AudioCallbackData` that is kept alive
///   by an `AudioDevice` for the device's entire lifetime.
/// - `p_output` must point to a buffer of at least `frame_count * 2 *
///   size_of::<f32>()` bytes (interleaved stereo).
#[cfg(feature = "audio")]
unsafe extern "C" fn audio_callback(
    p_device: *mut crate::miniaudio_bindings::ma_device,
    p_output: *mut std::ffi::c_void,
    _p_input: *const std::ffi::c_void,
    frame_count: crate::miniaudio_bindings::ma_uint32,
) {
    if p_output.is_null() || frame_count == 0 {
        return;
    }

    // SAFETY: pUserData was set during device initialization to point to the
    // AudioCallbackData. The AudioDevice struct holds an Arc to this data,
    // ensuring it remains valid for the device's entire lifetime.
    let data_ptr = unsafe { (*p_device).pUserData };
    if data_ptr.is_null() {
        return;
    }
    // SAFETY: data_ptr is valid for the device lifetime (see above).
    let data = unsafe { &*(data_ptr as *const AudioCallbackData) };

    let output = p_output as *mut f32;
    let total_samples = frame_count as usize * 2; // stereo interleaved

    // 1. Clear output buffer to silence.
    // SAFETY: output is valid for frame_count * 2 f32 samples (guaranteed by miniaudio).
    unsafe {
        std::ptr::write_bytes(output, 0, total_samples);
    }

    // 2. Render soundfont audio and mix into output.
    #[cfg(feature = "soundfont")]
    {
        let synth_guard = data.synth.lock().unwrap();
        if let Some(ref synth) = *synth_guard {
            let mut buffer = data.synth_buffer.lock().unwrap();
            buffer.resize(total_samples, 0.0);
            synth.render_audio(&mut buffer, frame_count as usize);
            // Drop the synth lock before mixing to hold locks briefly.
            drop(synth_guard);

            // SAFETY: output is valid for frame_count * 2 f32 samples.
            let output_slice = unsafe { std::slice::from_raw_parts_mut(output, total_samples) };
            for i in 0..total_samples {
                output_slice[i] += buffer[i];
            }
        }
    }

    // 3. Mix all currently playing samples (additive).
    {
        let mut playing = data.playing.lock().unwrap();
        // SAFETY: output is valid for frame_count * 2 f32 samples.
        let output_slice = unsafe { std::slice::from_raw_parts_mut(output, total_samples) };

        for sample in playing.iter_mut() {
            let remaining = &sample.data[sample.pos..];
            let to_mix = remaining.len().min(total_samples);
            for i in 0..to_mix {
                output_slice[i] += remaining[i];
            }
            sample.pos += to_mix;
        }

        // Remove finished samples.
        playing.retain(|s| s.pos < s.data.len());
    }
}

/// Initialize a miniaudio playback device with our data callback.
///
/// The device is initialized but NOT started. The caller should call
/// `AudioDevice::ensure_started()` before playing any audio.
///
/// Returns `Some(AudioDevice)` on success, `None` if the device could not
/// be opened or if `PT2_NO_AUDIO` environment variable is set (useful for
/// headless/CI environments where no audio hardware is available and ALSA
/// probing would otherwise block indefinitely).
#[cfg(feature = "audio")]
fn init_audio_device(callback_data: Arc<AudioCallbackData>) -> Option<AudioDevice> {
    // Skip device initialization in headless/CI environments.
    if std::env::var("PT2_NO_AUDIO").is_ok() {
        log::info!("Audio device initialization skipped (PT2_NO_AUDIO is set)");
        return None;
    }

    use crate::miniaudio_bindings::{
        ma_device, ma_device_config_init, ma_device_init,
        ma_device_type_ma_device_type_playback, ma_format_ma_format_f32,
        ma_result_MA_SUCCESS,
    };

    let data_ptr = Arc::as_ptr(&callback_data) as *mut std::ffi::c_void;

    // SAFETY: We provide a valid (stack-allocated, MaybeUninit) device pointer
    // and a valid config. The callback and user-data pointers are valid for
    // the lifetime of the returned AudioDevice.
    unsafe {
        let mut config = ma_device_config_init(ma_device_type_ma_device_type_playback);
        config.dataCallback = Some(audio_callback);
        config.pUserData = data_ptr;
        config.playback.format = ma_format_ma_format_f32;
        config.playback.channels = 2;
        config.sampleRate = 44100;

        let mut device = std::mem::MaybeUninit::<ma_device>::uninit();

        let result = ma_device_init(std::ptr::null_mut(), &config, device.as_mut_ptr());

        if result != ma_result_MA_SUCCESS {
            log::error!("Failed to initialize miniaudio device (result={})", result);
            return None;
        }

        let device = device.assume_init();

        // NOTE: We do NOT call ma_device_start() here. The device is started
        // lazily by AudioDevice::ensure_started() on the first play call.
        // This avoids a race condition in miniaudio's worker thread that can
        // cause an assertion failure (ma_device_state_starting) when starting
        // the device during AudioManager construction.
        log::info!("Miniaudio playback device initialized (lazy start)");

        Some(AudioDevice {
            device,
            _callback_data: callback_data,
            started: false,
        })
    }
}

// =============================================================================
// ActiveNote (non-feature-gated, used by MIDI playback logic)
// =============================================================================

/// Tracks a note that has been triggered (note_on) and is awaiting note_off.
struct ActiveNote {
    midi_number: u8,
    end_time: f64,
}

// =============================================================================
// AudioManager
// =============================================================================

pub struct AudioManager {
    midi_data: Option<MidiJson>,
    track_pointers: Vec<usize>,
    played_notes: HashSet<i64>,
    active_notes: Vec<ActiveNote>,
    /// When true, all audio output is suppressed (used during video recording).
    muted: bool,

    #[cfg(feature = "audio")]
    audio_device: Option<AudioDevice>,
    #[cfg(feature = "audio")]
    callback_data: Arc<AudioCallbackData>,
    #[cfg(feature = "audio")]
    samples: Arc<std::sync::RwLock<HashMap<String, DecodedSample>>>,
    #[cfg(feature = "audio")]
    sample_names: Vec<String>,
    #[cfg(feature = "audio")]
    assets_dir: Option<String>,

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
    soundfont_sample_rate: u32,
}

impl AudioManager {
    pub fn new() -> Self {
        #[cfg(feature = "audio")]
        let callback_data = Arc::new(AudioCallbackData {
            playing: Mutex::new(Vec::new()),
            synth_buffer: Mutex::new(Vec::new()),
            #[cfg(feature = "soundfont")]
            synth: Mutex::new(None),
        });

        // NOTE: Audio device initialization is DEFERRED to initialize_samples().
        // On Windows, ma_device_init() for the WASAPI backend calls
        // CoInitializeEx(COINIT_MULTITHREADED), which conflicts with winit's
        // OleInitialize() (which needs COINIT_APARTMENTTHREADED). By deferring
        // init to initialize_samples() (called from winit's resumed() handler),
        // we ensure the device is initialized AFTER the window is created.
        #[cfg(feature = "audio")]
        let audio_device: Option<AudioDevice> = None;

        Self {
            midi_data: None,
            track_pointers: Vec::new(),
            played_notes: HashSet::new(),
            active_notes: Vec::new(),
            muted: false,

            #[cfg(feature = "audio")]
            audio_device,
            #[cfg(feature = "audio")]
            callback_data,
            #[cfg(feature = "audio")]
            samples: Arc::new(std::sync::RwLock::new(HashMap::new())),
            #[cfg(feature = "audio")]
            sample_names: Vec::new(),
            #[cfg(feature = "audio")]
            assets_dir: None,

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
            soundfont_sample_rate: 44100,
        }
    }

    pub fn initialize_samples(&mut self, assets_dir: &str, preload: bool) {
        #[cfg(feature = "audio")]
        {
            // Initialize the audio device NOW (deferred from new()).
            // On Windows, ma_device_init() for WASAPI calls
            // CoInitializeEx(COINIT_MULTITHREADED). This must happen AFTER
            // winit has created its window, otherwise we get RPC_E_CHANGED_MODE.
            // initialize_samples() is always called from the winit resumed()
            // handler, ensuring the window exists before we touch the audio device.
            //
            // NOTE: We do NOT call ensure_started() here. The device start is
            // deferred to the first actual play call. This avoids a timing
            // sensitivity in miniaudio's WASAPI backend where the worker thread
            // can hit an assertion (ma_device_state_starting) if ma_device_start()
            // is called too soon after ma_device_init().
            if self.audio_device.is_none() {
                self.audio_device = init_audio_device(Arc::clone(&self.callback_data));
                if self.audio_device.is_none() {
                    log::warn!(
                        "Audio device not available; audio playback will be silent"
                    );
                }
            }

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
                                decode_mp3_file(&path)
                                    .map(|data| (name, DecodedSample { data }))
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
                // Extract the inner tsf handle from TinySFSynth so we can share
                // it between the main-thread API (note_on/note_off) and the
                // audio callback (render_audio).
                let handle = synth.get_synth().take();

                // Create a shared Arc to the raw handle.
                let handle_arc =
                    Arc::new(std::sync::Mutex::new(SendSyncTsfHandle::new(handle)));

                // Create two synth wrappers that share the same underlying handle:
                // - main_synth: stored in soundfont_synth for note_on/note_off from the game loop
                // - callback_synth: stored in callback_data for render_audio from the audio thread
                let main_synth = ArcSoundfontSynth::new(handle_arc.clone());
                let callback_synth = ArcSoundfontSynth::new(handle_arc);

                // Store in callback_data for audio rendering.
                let callback_arc: Arc<dyn SoundfontSynth + Send + Sync> =
                    Arc::new(callback_synth);
                *self.callback_data.synth.lock().unwrap() = Some(callback_arc);

                // Store in soundfont_synth for main-thread API calls.
                self.soundfont_synth = Some(Box::new(main_synth));

                self.soundfont_state = SoundfontState::Ready;
                self.using_soundfont = true;
                self.soundfont_path = Some(path_str);
                self.soundfont_playback_active = true;
                self.soundfont_sample_rate = 44100;

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
        }

        self.soundfont_synth = None;
        self.soundfont_state = SoundfontState::Uninitialized;
        self.using_soundfont = false;
        self.soundfont_path = None;

        // Clear the callback-side synth reference so the audio callback
        // stops rendering soundfont audio.
        #[cfg(feature = "audio")]
        {
            *self.callback_data.synth.lock().unwrap() = None;
        }

        log::info!("Soundfont unloaded");
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

    pub fn play_note_by_midi(&mut self, midi_number: u8) {
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

    pub fn play_sample(&mut self, filename: &str) {
        if self.muted {
            return;
        }
        #[cfg(feature = "audio")]
        {
            // Lazily start the audio device on first play call.
            // This ensures maximum time between ma_device_init() and
            // ma_device_start(), avoiding a timing sensitivity in
            // miniaudio's WASAPI backend on Windows.
            if let Some(ref mut device) = self.audio_device {
                if !device.is_started() {
                    device.ensure_started();
                }
            } else {
                return;
            }

            // First check if the sample is already loaded (lazy cache or pre-loaded)
            let is_loaded = self.samples.read().unwrap().contains_key(filename);

            if is_loaded {
                // Play from cache
                if let Ok(sample_map) = self.samples.read()
                    && let Some(sample) = sample_map.get(filename)
                {
                    log::debug!("Playing audio sample: {}", filename);
                    let playing = PlayingSample {
                        data: Arc::clone(&sample.data),
                        pos: 0,
                    };
                    self.callback_data.playing.lock().unwrap().push(playing);
                }
            } else if let Some(ref dir) = self.assets_dir {
                // Not loaded yet — decode on a background thread and play.
                let path = Path::new(dir).join("sounds/mp3/piano").join(filename);
                let filename_string = filename.to_string();
                let callback_data = Arc::clone(&self.callback_data);
                let samples_clone = Arc::clone(&self.samples);

                std::thread::spawn(move || {
                    if let Some(decoded_data) = decode_mp3_file(&path) {
                        log::debug!(
                            "Lazy loaded and decoded audio sample in background: {}",
                            filename_string
                        );

                        // Cache it
                        let sample = DecodedSample {
                            data: Arc::clone(&decoded_data),
                        };
                        if let Ok(mut map) = samples_clone.write() {
                            map.insert(filename_string.clone(), sample);
                        }

                        // Instantly queue for playback
                        let playing = PlayingSample {
                            data: decoded_data,
                            pos: 0,
                        };
                        callback_data.playing.lock().unwrap().push(playing);
                    } else {
                        log::warn!(
                            "Failed to decode lazily loaded file: {}",
                            filename_string
                        );
                    }
                });
            } else {
                log::warn!("Sample not found and no assets dir: {}", filename);
            }
        }
    }

    pub fn play_random_sample(&mut self) {
        if self.muted {
            return;
        }
        #[cfg(feature = "audio")]
        {
            if self.midi_data.is_some() {
                return;
            }

            // Check that the device exists and is started (lazy init).
            if self.audio_device.as_ref().map_or(false, |d| d.is_started()) {
                if !self.sample_names.is_empty() {
                    use rand::Rng;
                    let mut rng = rand::thread_rng();
                    let random_index = rng.gen_range(0..self.sample_names.len());
                    if let Some(filename) = self.sample_names.get(random_index).cloned() {
                        log::debug!("Playing random sample: {}", filename);
                        self.play_sample(&filename);
                    }
                } else {
                    log::warn!("No samples available to play random sound");
                }
            }
        }
    }

    pub fn play_game_over_chord(&mut self) {
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
            // Clear all currently playing samples. The audio callback will see
            // an empty vec and produce silence.
            self.callback_data.playing.lock().unwrap().clear();
        }

        #[cfg(feature = "soundfont")]
        {
            // Stop all synthesizer notes
            if let Some(ref synth) = self.soundfont_synth {
                synth.reset();
            }
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
