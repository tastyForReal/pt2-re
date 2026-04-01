# Soundfont (.sf2) Integration Design Document

**Project:** Piano Tiles Re:U  
**Author:** Architecture Design  
**Date:** 2026-03-29  
**Version:** 1.0  

---

## 1. Executive Summary

This document outlines the architectural design for integrating SoundFont (.sf2) synthesis support into the existing audio pipeline. The integration adds a software synthesizer option alongside the existing MP3 sample playback, with automatic fallback to MP3 samples when soundfont loading fails.

### Key Design Decisions

1. **Synthesis Engine:** Use `fluidlite` crate (lightweight FluidSynth bindings) over full `fluidsynth` for smaller binary size and safer Rust API
2. **Integration Pattern:** Wrap fluidlite synthesizer output as a rodio `Source` to integrate seamlessly with existing DynamicMixer
3. **Fallback Strategy:** Automatic fallback to MP3 samples without user intervention; explicit opt-out via CLI flag
4. **Precedence Rule:** CLI argument > configuration file > built-in default (`assets/sounds/sf2/piano.sf2`)

---

## 2. Architectural Overview

### 2.1 Audio Pipeline Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        AudioManager                                  │
├─────────────────────────────────────────────────────────────────────┤
│  ┌──────────────────┐    ┌──────────────────┐                     │
│  │  MP3 Sample      │    │  Soundfont        │                     │
│  │  Playback        │    │  Synthesizer      │                     │
│  │  (rodio)         │    │  (fluidlite)      │                     │
│  └────────┬─────────┘    └────────┬─────────┘                     │
│           │                        │                                 │
│           └──────────┬─────────────┘                                 │
│                      ▼                                               │
│           ┌──────────────────┐                                       │
│           │  DynamicMixer   │◄──── Unified audio routing            │
│           │  (rodio)        │                                        │
│           └────────┬─────────┘                                       │
│                    ▼                                                 │
│           ┌──────────────────┐                                       │
│           │  Audio Output    │                                        │
│           │  (rodio Stream)  │                                        │
│           └──────────────────┘                                       │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 Event Flow for Note Playback

```
update_midi_playback() 
    │
    ▼
┌────────────────────────────────────────────┐
│  Check: Is soundfont enabled & loaded?     │
└──────────────┬─────────────────────────────┘
               │
       ┌───────┴───────┐
       ▼               ▼
      YES             NO
       │               │
       ▼               ▼
┌──────────────┐  ┌────────────────────┐
│ fluidlite    │  │ MP3 Sample         │
│ note_on()    │  │ play_sample()      │
└──────────────┘  └────────────────────┘
       │               │
       └───────┬───────┘
               ▼
      ┌────────────────┐
      │ DynamicMixer  │
      │ add_source()  │
      └────────────────┘
```

---

## 3. Crate Selection and Justification

### 3.1 Recommended Crate: `fluidlite`

| Criteria | fluidlite | fluidsynth | rustysynth |
|-----------|-----------|-------------|------------|
| Binary size | ~500KB | ~2MB | ~100KB |
| Rust API safety | High (safe bindings) | Medium | High |
| SF2/SF3 support | Yes | Yes | Yes |
| MIDI support | Yes | Yes | Yes |
| Active maintenance | Yes | Yes | Yes |
| Windows support | Yes | Yes | Yes |
| macOS support | Yes | Yes | Yes |
| Linux support | Yes | Yes | Yes |

**Recommendation:** Use `fluidlite` for the balance of:
- Small memory footprint (~10-20MB at runtime vs ~50MB for full FluidSynth)
- Safe Rust bindings (no `unsafe` required in application code)
- Cross-platform support matching existing rodio compatibility

### 3.2 Cargo.toml Changes

```toml
[features]
default = ["audio"]
audio = ["dep:rodio"]
soundfont = ["dep:fluidlite", "dep:rodio"]  # New feature gate

[dependencies]
# ... existing deps ...
rodio = { version = "0.20", optional = true, features = ["mp3"] }

# New soundfont synthesis dependency
fluidlite = "0.2"
```

**Feature Gating Strategy:**
- `soundfont` feature enables fluidlite integration
- When `soundfont` is enabled but fails to load, falls back to MP3 automatically
- When `soundfont` is disabled at compile time, only MP3 path is available

---

## 4. API Surface Changes

### 4.1 New Module: `src/game/soundfont_manager.rs`

```rust
use std::sync::Arc;
use std::path::Path;

/// Result type for soundfont operations
pub type SoundfontResult<T> = Result<T, SoundfontError>;

/// Soundfont-specific errors
#[derive(Debug, Clone)]
pub enum SoundfontError {
    /// Soundfont file not found
    FileNotFound(String),
    /// Failed to parse SF2 file
    ParseError(String),
    /// Failed to initialize synthesizer
    SynthInitError(String),
    /// MIDI event processing error
    MidiError(String),
    /// Audio output error
    AudioError(String),
    /// Soundfont disabled at compile time
    NotEnabled,
}

impl std::fmt::Display for SoundfontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotFound(path) => write!(f, "Soundfont file not found: {}", path),
            Self::ParseError(msg) => write!(f, "Failed to parse soundfont: {}", msg),
            Self::SynthInitError(msg) => write!(f, "Synthesizer initialization failed: {}", msg),
            Self::MidiError(msg) => write!(f, "MIDI error: {}", msg),
            Self::AudioError(msg) => write!(f, "Audio output error: {}", msg),
            Self::NotEnabled => write!(f, "Soundfont support not compiled in"),
        }
    }
}

impl std::error::Error for SoundfontError {}

/// Configuration for soundfont playback
#[derive(Clone, Debug)]
pub struct SoundfontConfig {
    /// Path to the soundfont file
    pub path: String,
    /// Sample rate for synthesis (default: 44100)
    pub sample_rate: u32,
    /// Number of voices (polyphony) - default: 256
    pub voices: u32,
    /// Enable/disable reverb effect (default: true)
    pub reverb: bool,
    /// Enable/disable chorus effect (default: false)
    pub chorus: bool,
    /// Master volume (0.0 - 1.0, default: 0.8)
    pub volume: f32,
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
        }
    }
}

/// Internal state of the soundfont synthesizer
pub enum SoundfontState {
    /// Not initialized
    Uninitialized,
    /// Currently loading
    Loading,
    /// Successfully loaded and ready
    Ready,
    /// Failed to load, fallback to MP3
    Fallback,
    /// Explicitly disabled by user
    Disabled,
}

/// Trait for soundfont synthesis operations
#[cfg(feature = "soundfont")]
pub trait SoundfontSynth: Send + Sync {
    /// Play a MIDI note
    fn note_on(&self, channel: u8, note: u8, velocity: u8);
    
    /// Stop a MIDI note
    fn note_off(&self, channel: u8, note: u8);
    
    /// Stop all notes
    fn all_notes_off(&self);
    
    /// Check if synthesizer is ready
    fn is_ready(&self) -> bool;
    
    /// Get current state
    fn state(&self) -> SoundfontState;
}
```

### 4.2 AudioManager Extensions

```rust
/// Extended AudioManager with soundfont support
pub struct AudioManager {
    // ... existing fields ...
    
    // === NEW FIELDS ===
    
    /// Soundfont configuration
    #[cfg(feature = "soundfont")]
    soundfont_config: Option<SoundfontConfig>,
    
    /// Soundfont synthesizer instance
    #[cfg(feature = "soundfont")]
    soundfont_synth: Option<Arc<dyn SoundfontSynth>>,
    
    /// Current soundfont state
    #[cfg(feature = "soundfont")]
    soundfont_state: SoundfontState,
    
    /// Flag to enable/disable fallback to MP3
    #[cfg(feature = "soundfont")]
    fallback_enabled: bool,
    
    /// Flag indicating if we're currently using soundfont
    #[cfg(feature = "soundfont")]
    using_soundfont: bool,
}
```

### 4.3 New Methods on AudioManager

```rust
impl AudioManager {
    // === NEW METHODS ===
    
    /// Initialize soundfont with the given configuration
    /// Returns Ok if soundfont loads successfully, error otherwise
    #[cfg(feature = "soundfont")]
    pub fn init_soundfont(&mut self, config: SoundfontConfig) -> SoundfontResult<()> {
        // 1. Validate path exists
        // 2. Load SF2 file into fluidlite
        // 3. Initialize synthesizer with config
        // 4. Set state to Ready or Fallback
    }
    
    /// Load soundfont from a path string (convenience wrapper)
    #[cfg(feature = "soundfont")]
    pub fn load_soundfont(&mut self, path: &str) -> SoundfontResult<()> {
        let config = SoundfontConfig {
            path: path.to_string(),
            ..Default::default()
        };
        self.init_soundfont(config)
    }
    
    /// Set fallback behavior (default: true)
    #[cfg(feature = "soundfont")]
    pub fn set_fallback_enabled(&mut self, enabled: bool) {
        self.fallback_enabled = enabled;
    }
    
    /// Get current soundfont state
    #[cfg(feature = "soundfont")]
    pub fn soundfont_state(&self) -> SoundfontState {
        self.soundfont_state.clone()
    }
    
    /// Check if soundfont is currently active
    #[cfg(feature = "soundfont")]
    pub fn is_using_soundfont(&self) -> bool {
        self.using_soundfont
    }
    
    /// Unload soundfont and release resources
    #[cfg(feature = "soundfont")]
    pub fn unload_soundfont(&mut self) {
        if let Some(ref synth) = self.soundfont_synth {
            synth.all_notes_off();
        }
        self.soundfont_synth = None;
        self.soundfont_state = SoundfontState::Uninitialized;
        self.using_soundfont = false;
        log::info!("Soundfont unloaded");
    }
    
    /// Reconfigure soundfont at runtime
    #[cfg(feature = "soundfont")]
    pub fn reload_soundfont(&mut self, new_path: &str) -> SoundfontResult<()> {
        self.unload_soundfont();
        self.load_soundfont(new_path)
    }
    
    /// Play note via soundfont (new method)
    #[cfg(feature = "soundfont")]
    pub fn play_note_via_soundfont(&self, midi_number: u8, velocity: u8) {
        if self.using_soundfont {
            if let Some(ref synth) = self.soundfont_synth {
                // Map MIDI 21-108 to channel 0
                synth.note_on(0, midi_number, velocity);
            }
        }
    }
    
    /// Stop note via soundfont
    #[cfg(feature = "soundfont")]
    pub fn stop_note_via_soundfont(&self, midi_number: u8) {
        if self.using_soundfont {
            if let Some(ref synth) = self.soundfont_synth {
                synth.note_off(0, midi_number);
            }
        }
    }
    
    /// Get diagnostic info for logging
    #[cfg(feature = "soundfont")]
    pub fn get_soundfont_info(&self) -> String {
        format!("Soundfont: state={:?}, fallback={}, active={}",
            self.soundfont_state,
            self.fallback_enabled,
            self.using_soundfont
        )
    }
}
```

---

## 5. CLI Integration (src/main.rs)

### 5.1 New CLI Arguments

```rust
#[derive(Parser, Debug)]
#[command(name = "pt2")]
struct Cli {
    // ... existing fields ...
    
    /// Path to a SoundFont (.sf2) file for synthesis playback.
    /// Default: assets/sounds/sf2/piano.sf2
    /// Precedence: CLI > built-in default
    #[arg(long, value_name = "PATH")]
    soundfont: Option<String>,
    
    /// Disable automatic fallback to MP3 samples when soundfont fails to load.
    /// When specified, errors during soundfont loading will cause the game
    /// to run without audio rather than falling back to MP3.
    #[arg(long, default_value_t = true)]
    soundfont_fallback: bool,
    
    /// Prefer soundfont synthesis over MP3 samples when available.
    /// This is the default behavior; use --no-soundfont to disable synthesis.
    #[arg(long, default_value_t = true)]
    soundfont_enabled: bool,
}
```

### 5.2 Path Resolution Precedence

```
┌─────────────────────────────────────────────────────────────┐
│              Soundfont Path Resolution                      │
├─────────────────────────────────────────────────────────────┤
│  1. CLI argument: --soundfont /custom/path/piano.sf2      │
│     ↓ (if not provided)                                     │
│  2. Built-in default: assets/sounds/sf2/piano.sf2          │
└─────────────────────────────────────────────────────────────┘

Resolution algorithm:
1. If --soundfont is explicitly provided:
   a. Check if file exists → use it
   b. If not exists → log error, proceed to fallback
   
2. If --soundfont not provided:
   a. Try default path relative to executable
   b. Try default path relative to current working directory
   c. If not found → log warning, use fallback
```

### 5.3 Cross-Platform Path Handling

```rust
/// Resolve soundfont path with cross-platform support
fn resolve_soundfont_path(cli_path: Option<&str>, exe_dir: &Path) -> PathBuf {
    // Priority 1: CLI-provided path
    if let Some(path) = cli_path {
        let p = PathBuf::from(path);
        if p.exists() {
            return p;
        }
        // Try relative to executable
        let p2 = exe_dir.join(path);
        if p2.exists() {
            return p2;
        }
        log::warn!("CLI soundfont path not found: {}", path);
    }
    
    // Priority 2: Built-in default
    let default_relative = exe_dir.join("assets/sounds/sf2/piano.sf2");
    if default_relative.exists() {
        return default_relative;
    }
    
    // Priority 3: Fallback to current directory
    let cwd_default = PathBuf::from("assets/sounds/sf2/piano.sf2");
    if cwd_default.exists() {
        return cwd_default;
    }
    
    // Return default path even if doesn't exist (for error reporting)
    default_relative
}
```

### 5.4 Integration in App::new()

```rust
impl App {
    fn new(cli: Cli) -> Self {
        // ... existing code ...
        
        // Determine soundfont path
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        
        let soundfont_path = resolve_soundfont_path(cli.soundfont.as_deref(), &exe_dir);
        
        // Store soundfont config for initialization
        // (passed to audio manager later)
        
        Self {
            // ... existing fields ...
            cli,
            soundfont_path,  // NEW
            // ...
        }
    }
}
```

### 5.5 Initialization in resumed()

```rust
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ... existing initialization ...
        
        // Initialize audio manager with soundfont support
        #[cfg(feature = "soundfont")]
        {
            if self.cli.soundfont_enabled {
                match self.game_controller
                    .audio_manager
                    .load_soundfont(&self.soundfont_path.to_string_lossy())
                {
                    Ok(()) => {
                        log::info!("Soundfont loaded successfully: {:?}", self.soundfont_path);
                        self.game_controller
                            .audio_manager
                            .set_fallback_enabled(self.cli.soundfont_fallback);
                    }
                    Err(e) => {
                        log::error!("Failed to load soundfont: {}", e);
                        if !self.cli.soundfont_fallback {
                            log::warn!("Fallback disabled; audio may not play");
                        } else {
                            log::info!("Falling back to MP3 sample playback");
                        }
                    }
                }
            } else {
                log::info!("Soundfont synthesis disabled via CLI");
            }
        }
        
        // Continue with existing initialization...
    }
}
```

---

## 6. Runtime Fallback Strategy

### 6.1 Fallback Decision Flow

```
┌─────────────────────────────────────────────────────────────┐
│              Soundfont Fallback Flow                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Soundfont Loading                                           │
│       │                                                      │
│       ▼                                                      │
│  ┌─────────┐     YES      ┌─────────────┐                  │
│  │ Success │ ──────────►  │ Use Synth   │                  │
│  └────┬────┘              └─────────────┘                  │
│       │ NO                                                       │
│       ▼                                                      │
│  ┌─────────────────┐                                        │
│  │ Load Failed     │                                        │
│  └────────┬────────┘                                        │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────┐     NO        ┌────────────────┐       │
│  │ Fallback        │ ──────────►  │ No Audio       │       │
│  │ Enabled?        │               │ (continue)     │       │
│  └────────┬────────┘               └────────────────┘       │
│           │ YES                                               │
│           ▼                                                   │
│  ┌─────────────────┐                                        │
│  │ Use MP3 Samples│                                        │
│  │ (continue)      │                                        │
│  └─────────────────┘                                        │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 Modified play_note_by_midi()

```rust
pub fn play_note_by_midi(&self, midi_number: u8) {
    // Existing MP3 fallback path maintained for:
    // - soundfont feature disabled at compile time
    // - soundfont not enabled at runtime
    // - fallback to MP3 after soundfont failure
    
    #[cfg(feature = "soundfont")]
    {
        // Check if soundfont is active
        if self.using_soundfont {
            // Use synthesis with velocity based on game state
            // Default velocity: 100 (out of 127)
            self.play_note_via_soundfont(midi_number, 100);
            return;
        }
    }
    
    // Fallback to existing MP3 playback
    if let Some(note_name) = midi_to_note(midi_number) {
        let filename = format!("{}.mp3", note_name);
        self.play_sample(&filename);
    }
}
```

### 6.3 Error Handling and Logging

```rust
// Example log messages for various scenarios:

// Success
log::info!("Soundfont loaded: {} ({} voices)", path, config.voices);

// File not found
log::error!("Soundfont file not found: {}. Falling back to MP3.", path);

// Parse error
log::error!("Invalid soundfont format: {}. Falling back to MP3.", path);

// Fallback disabled
log::error!("Soundfont failed to load and fallback is disabled. Running without audio.");

// Runtime switch
log::info!("Switching from soundfont to MP3 samples");
log::info!("Switching from MP3 to soundfont synthesis");
```

---

## 7. Resource Management

### 7.1 Lifecycle Overview

```
┌─────────────────────────────────────────────────────────────┐
│              Soundfont Resource Lifecycle                   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Application Start                                           │
│       │                                                      │
│       ▼                                                      │
│  ┌─────────────────────────────────────────┐                 │
│  │ AudioManager::new()                     │                 │
│  │   - State: Uninitialized                │                 │
│  └─────────────────────────────────────────┘                 │
│       │                                                      │
│       ▼                                                      │
│  ┌─────────────────────────────────────────┐                 │
│  │ App::resumed()                          │                 │
│  │   - AudioManager::load_soundfont()      │                 │
│  │   - State: Loading → Ready/Fallback     │                 │
│  └─────────────────────────────────────────┘                 │
│       │                                                      │
│       ▼                                                      │
│  ┌─────────────────────────────────────────┐                 │
│  │ Game Loop                               │                 │
│  │   - play_note_by_midi()                │                 │
│  │   - Runtime reconfigure (if requested) │                 │
│  └─────────────────────────────────────────┘                 │
│       │                                                      │
│       ▼                                                      │
│  ┌─────────────────────────────────────────┐                 │
│  │ Application Shutdown                    │                 │
│  │   - AudioManager::unload_soundfont()    │                 │
│  │   - All notes off                       │                 │
│  │   - Release synthesizer resources      │                 │
│  └─────────────────────────────────────────┘                 │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 7.2 Shutdown Handler

```rust
// In audio_manager.rs or main.rs cleanup

impl Drop for AudioManager {
    fn drop(&mut self) {
        #[cfg(feature = "soundfont")]
        {
            self.unload_soundfont();
        }
        // Existing audio cleanup continues...
    }
}
```

### 7.3 Memory and CPU Considerations

| Aspect | MP3 Samples | Soundfont Synthesis |
|--------|-------------|---------------------|
| **Memory (idle)** | ~0 MB | ~10-20 MB |
| **Memory (playing)** | + decoded samples | + voice allocation |
| **CPU (per note)** | ~1% (decode) | ~2-5% (synthesis) |
| **Latency** | 50-100ms | 5-20ms |
| **Disk I/O** | On load | None (in memory) |

**Recommendations:**
1. Default to MP3 for lower-end systems
2. Offer soundfont as opt-in for better latency
3. Use lazy loading for soundfont to not block startup
4. Limit polyphony voices (default 256 is sufficient for piano)

---

## 8. Testing Plan

### 8.1 Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    // === Soundfont Path Resolution Tests ===
    
    #[test]
    fn test_resolve_soundfont_cli_path_absolute() {
        // Test absolute path resolution
    }
    
    #[test]
    fn test_resolve_soundfont_cli_path_relative() {
        // Test relative to executable
    }
    
    #[test]
    fn test_resolve_soundfont_default_exists() {
        // Test default path fallback
    }
    
    #[test]
    fn test_resolve_soundfont_cross_platform() {
        // Test Windows/macOS/Linux path handling
    }
    
    // === Soundfont State Tests ===
    
    #[test]
    fn test_soundfont_state_transitions() {
        // Test Uninitialized → Loading → Ready
        // Test Loading → Fallback on error
    }
    
    #[test]
    fn test_soundfont_fallback_enabled() {
        // Test fallback behavior
    }
    
    #[test]
    fn test_soundfont_fallback_disabled() {
        // Test no-fallback behavior
    }
    
    // === MIDI Mapping Tests ===
    
    #[test]
    fn test_midi_note_range() {
        // Test notes 21-108 map correctly
    }
    
    #[test]
    fn test_midi_velocity_handling() {
        // Test velocity values are clamped
    }
}
```

### 8.2 Integration Tests

```rust
#[cfg(feature = "soundfont")]
mod integration_tests {
    use super::*;
    
    #[test]
    fn test_soundfont_load_and_play() {
        // 1. Load soundfont from test fixture
        // 2. Play a note
        // 3. Verify audio is produced
    }
    
    #[test]
    fn test_soundfont_to_mp3_fallback() {
        // 1. Attempt to load invalid soundfont
        // 2. Verify fallback to MP3
        // 3. Play note via MP3
    }
    
    #[test]
    fn test_runtime_soundfont_reload() {
        // 1. Load soundfont
        // 2. Reload with new path
        // 3. Verify new soundfont works
    }
    
    #[test]
    fn test_concurrent_playback() {
        // 1. Play multiple notes simultaneously
        // 2. Verify no audio glitches
    }
    
    #[test]
    fn test_note_on_note_off() {
        // 1. Note on
        // 2. Note off
        // 3. Verify note stops
    }
}
```

### 8.3 Test Scenarios

| Test Case | Expected Result | Verification |
|-----------|------------------|---------------|
| Valid SF2 loads | Soundfont active | `is_using_soundfont() == true` |
| Invalid SF2 path | Fallback to MP3 | Log warning, MP3 plays |
| Malformed SF2 file | Fallback to MP3 | Log error, MP3 plays |
| Fallback disabled + error | No audio | Log error, silent |
| Switch SF2 at runtime | New SF2 active | `soundfont_path` updated |
| Shutdown with SF2 loaded | Clean unload | No memory leak |

---

## 9. Implementation Tasks

### Phase 1: Core Infrastructure (Week 1)

1. **Update Cargo.toml**
   - Add `fluidlite` dependency
   - Add `soundfont` feature flag

2. **Create soundfont_manager.rs**
   - Implement `SoundfontConfig`
   - Implement `SoundfontError`
   - Implement `FluidLiteSynth` struct

3. **Add CLI arguments**
   - `--soundfont`, `--soundfont-fallback`, `--soundfont-enabled`

### Phase 2: Integration (Week 2)

4. **Modify audio_manager.rs**
   - Add new fields
   - Implement `load_soundfont()`, `unload_soundfont()`
   - Modify `play_note_by_midi()` for dual path
   - Implement fallback logic

5. **Update main.rs**
   - Path resolution logic
   - Initialization in `App::resumed()`

### Phase 3: Testing and Polish (Week 3)

6. **Unit tests**
   - Path resolution tests
   - State machine tests

7. **Integration tests**
   - End-to-end playback test
   - Fallback behavior tests

8. **Documentation**
   - Update README with new CLI options
   - Add troubleshooting guide

---

## 10. Acceptance Criteria

### Functional Requirements

- [ ] Soundfont loads from default path `assets/sounds/sf2/piano.sf2`
- [ ] CLI `--soundfont /path/to/file.sf2` overrides default path
- [ ] Path resolution works on Windows, macOS, and Linux
- [ ] MIDI notes 21-108 play correctly via synthesizer
- [ ] Invalid soundfont triggers automatic fallback to MP3
- [ ] `--no-soundfont-fallback` disables automatic fallback
- [ ] Runtime reconfiguration via `reload_soundfont()` works
- [ ] Clean shutdown with proper resource cleanup

### Performance Requirements

- [ ] Soundfont loading does not block main thread
- [ ] Note latency < 50ms
- [ ] Memory usage < 50MB with soundfont loaded
- [ ] No audio glitches during polyphonic playback

### Error Handling Requirements

- [ ] Clear error messages for missing soundfont file
- [ ] Clear error messages for malformed soundfont
- [ ] Logging of fallback activation
- [ ] No crashes on soundfont errors (graceful degradation)

### Compatibility Requirements

- [ ] Works with existing MP3 sample infrastructure
- [ ] Compatible with rodio DynamicMixer
- [ ] Cross-platform builds work (Windows, macOS, Linux)

---

## 11. Risk Assessment and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| fluidlite FFI crashes | Low | High | Wrap in panic-free boundary; fallback on any error |
| Soundfont memory bloat | Medium | Medium | Limit voices, lazy load, provide disable option |
| Audio glitches on fallback | Low | Low | Keep MP3 path as proven fallback |
| Cross-platform path issues | Medium | Medium | Extensive testing on all platforms |
| Build failures on some platforms | Medium | High | Feature-gated, disable gracefully |
| MIDI timing issues | Low | Medium | Test with various polyphony levels |

---

## 12. Migration Path

### For Existing Builds

1. **No Breaking Changes**
   - Default behavior unchanged (MP3 playback)
   - Soundfont opt-in via `--soundfont` flag
   - Existing CLI args work unchanged

2. **Stepwise Adoption**
   - Phase 1: Compile with `--features soundfont`
   - Phase 2: Add `piano.sf2` to assets (already present)
   - Phase 3: Enable via CLI or config
   - Phase 4: (Optional) Make soundfont default

### For Users

1. **Automatic (if soundfont present)**
   - Game detects `assets/sounds/sf2/piano.sf2`
   - Uses soundfont automatically

2. **Manual (custom soundfont)**
   - `--soundfont /path/to/custom.sf2`

3. **Opt-out**
   - `--no-soundfont-enabled`

---

## 13. Appendix: Example Usage

### CLI Examples

```bash
# Use default soundfont (if exists)
./pt2 --file levels/J/Jingle\ Bells.json

# Use custom soundfont
./pt2 --file levels/J/Jingle\ Bells.json --soundfont ~/sounds/grand-piano.sf2

# Disable soundfont, use MP3 only
./pt2 --file levels/J/Jingle\ Bells.json --no-soundfont-enabled

# Disable fallback (error if soundfont fails)
./pt2 --file levels/J/Jingle\ Bells.json --soundfont invalid.sf2 --no-soundfont-fallback
```

### Configuration File (Future Enhancement)

```json
{
  "audio": {
    "soundfont": {
      "path": "assets/sounds/sf2/piano.sf2",
      "enabled": true,
      "fallback": true,
      "voices": 256,
      "volume": 0.8
    }
  }
}
```

---

## 14. Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-03-29 | Architecture | Initial design document |

---

*End of Document*
