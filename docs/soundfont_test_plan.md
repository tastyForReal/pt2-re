# Soundfont Integration Test Plan

**Project:** Piano Tiles Re:U  
**Feature:** SoundFont (.sf2) Synthesis Integration  
**Date:** 2026-03-29  
**Version:** 1.0  

---

## 1. Overview

This test plan defines comprehensive testing for the soundfont integration feature, covering unit tests, integration tests, fallback behavior verification, and cross-platform validation.

### 1.1 Test Objectives

1. **Verify soundfont loading** - Valid SF2 files load correctly
2. **Verify MIDI playback** - Notes play through synthesizer
3. **Verify fallback behavior** - MP3 fallback works when soundfont fails
4. **Verify runtime switching** - Can switch between soundfont and MP3
5. **Verify resource cleanup** - Proper shutdown without memory leaks
6. **Verify cross-platform paths** - Paths resolve correctly on all platforms

---

## 2. Test Categories

### 2.1 Unit Tests

#### 2.1.1 Path Resolution Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| UT-PR-01 | `test_resolve_sf_cli_absolute_path` | CLI provides absolute Windows path | Resolves to that exact path |
| UT-PR-02 | `test_resolve_sf_cli_absolute_unix` | CLI provides absolute Unix path | Resolves to that exact path |
| UT-PR-03 | `test_resolve_sf_cli_relative_to_exe` | CLI provides relative path, exists in exe dir | Resolves to exe-relative path |
| UT-PR-04 | `test_resolve_sf_cli_relative_cwd` | CLI provides relative path, exists in cwd | Resolves to cwd-relative path |
| UT-PR-05 | `test_resolve_sf_nonexistent_cli_path` | CLI provides path that doesn't exist | Returns path with "cli_not_found" source |
| UT-PR-06 | `test_resolve_sf_default_with_no_cli` | No CLI path, default exists | Returns default path |
| UT-PR-07 | `test_resolve_sf_default_not_found` | No CLI path, default doesn't exist | Returns default path with "default_not_found" source |

#### 2.1.2 Soundfont Config Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| UT-SC-01 | `test_config_default_values` | Create default config | sample_rate=44100, voices=256, reverb=true, volume=0.8 |
| UT-SC-02 | `test_config_custom_path` | Create config with custom path | path equals provided value |
| UT-SC-03 | `test_config_builder_pattern` | Use builder methods | All settings applied correctly |
| UT-SC-04 | `test_config_volume_clamp_high` | Set volume > 1.0 | Clamped to 1.0 |
| UT-SC-05 | `test_config_volume_clamp_low` | Set volume < 0.0 | Clamped to 0.0 |

#### 2.1.3 Soundfont State Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| UT-SS-01 | `test_state_default_uninitialized` | Default state after creation | Uninitialized |
| UT-SS-02 | `test_state_transitions` | Simulate state changes | Correct state at each step |
| UT-SS-03 | `test_state_clone` | Clone state enum | Independent copy |

#### 2.1.4 Validation Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| UT-VA-01 | `test_validate_path_valid_file` | Valid SF2 file | Ok(PathBuf) |
| UT-VA-02 | `test_validate_path_not_found` | Non-existent file | Err(FileNotFound) |
| UT-VA-03 | `test_validate_path_empty_file` | Empty file | Err(ParseError) |
| UT-VA-04 | `test_validate_path_too_small` | File < 1KB | Warn log, but Ok (allows tiny fonts) |

#### 2.1.5 Error Handling Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| UT-EH-01 | `test_error_display_file_not_found` | Error.to_string() | Contains path |
| UT-EH-02 | `test_error_display_not_enabled` | Error.to_string() | Contains "not compiled" |

---

### 2.2 Integration Tests

#### 2.2.1 Soundfont Loading Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| IT-SL-01 | `test_load_valid_soundfont` | Load `piano.sf2` | state=Ready, using_soundfont=true |
| IT-SL-02 | `test_load_invalid_path` | Load non-existent file | state=Fallback, MP3 active |
| IT-SL-03 | `test_load_twice_error` | Call load_soundfont twice | Err(AlreadyLoaded) |
| IT-SL-04 | `test_unload_then_reload` | Unload then reload different file | Both operations succeed |

#### 2.2.2 MIDI Playback Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| IT-MP-01 | `test_play_note_soundfont` | play_note_by_midi(60) with SF active | Note plays via synthesizer |
| IT-MP-02 | `test_play_note_out_of_range` | play_note_by_midi(127) | Ignored (MIDI 21-108 valid) |
| IT-MP-03 | `test_play_note_fallback_mp3` | play_note_by_midi(60) in fallback | MP3 sample plays |
| IT-MP-04 | `test_all_notes_off` | Call all_notes_off | All synth notes stop |

#### 2.2.3 Fallback Behavior Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| IT-FB-01 | `test_fallback_enabled_on_error` | Invalid SF with fallback=true | MP3 playback works |
| IT-FB-02 | `test_fallback_disabled_on_error` | Invalid SF with fallback=false | No audio |
| IT-FB-03 | `test_fallback_recovery` | Start with bad SF, reload good SF | Soundfont becomes active |
| IT-FB-04 | `test_fallback_logged` | Fallback activation | Warning log contains "fallback" |

#### 2.2.4 Runtime Configuration Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| IT-RC-01 | `test_toggle_soundfont_enabled` | set_soundfont_enabled(false) | using_soundfont=false |
| IT-RC-02 | `test_reload_soundfont` | reload_soundfont("other.sf2") | New SF active |
| IT-RC-03 | `test_get_soundfont_info` | Call get_soundfont_info() | String contains state info |

#### 2.2.5 Lifecycle Tests

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| IT-LC-01 | `test_drop_cleanup` | Drop AudioManager with SF loaded | No panic, no leak |
| IT-LC-02 | `test_reset_clears_notes` | reset() after playing notes | All notes stopped |
| IT-LC-03 | `test_reset_playback_clears_sf` | reset_playback() calls synth.all_notes_off() | All synth notes stopped |

---

### 2.3 End-to-End Tests

#### 2.3.1 Game Loop Integration

| Test ID | Test Name | Description | Expected Result |
|---------|-----------|-------------|-----------------|
| IT-E2E-01 | `test_game_loop_with_soundfont` | Play level with SF active | Notes play correctly |
| IT-E2E-02 | `test_game_loop_fallback` | SF fails, MP3 used | Game continues normally |
| IT-E2E-03 | `test_concurrent_notes` | Multiple simultaneous notes | No audio glitches |

---

### 2.4 Platform-Specific Tests

| Test ID | Test Name | Platform | Description |
|---------|-----------|----------|-------------|
| PL-WIN-01 | `test_windows_path_separators` | Windows | SF path with backslashes works |
| PL-MAC-01 | `test_macos_path_separators` | macOS | SF path with forward slashes works |
| PL-LIN-01 | `test_linux_path_separators` | Linux | SF path with forward slashes works |
| PL-ALL-01 | `test_default_path_resolution` | All | Default `assets/sounds/sf2/piano.sf2` resolves |

---

## 3. Test Fixtures

### 3.1 Soundfont Test Files

```
tests/fixtures/
├── soundfonts/
│   ├── piano.sf2           # Valid piano soundfont (exists in assets)
│   ├── empty.sf2           # Empty file (invalid)
│   ├── corrupted.sf2       # File with wrong magic bytes
│   └── custom.sf2         # Alternative soundfont for reload tests
```

### 3.2 Test Data

```rust
// Example test constants
const TEST_SF_VALID = "tests/fixtures/soundfonts/piano.sf2";
const TEST_SF_INVALID = "tests/fixtures/soundfonts/empty.sf2";
const TEST_SF_CORRUPTED = "tests/fixtures/soundfonts/corrupted.sf2";
const TEST_SF_CUSTOM = "tests/fixtures/soundfonts/custom.sf2";
const TEST_MIDI_NOTE = 60;  // Middle C
```

---

## 4. Test Infrastructure

### 4.1 Test Utilities

```rust
// tests/integration/helpers.rs

/// Helper to create AudioManager with soundfont feature
#[cfg(feature = "soundfont")]
fn create_test_audio_manager() -> AudioManager {
    AudioManager::new()
}

/// Helper to load test soundfont
#[cfg(feature = "soundfont")]
fn load_test_soundfont(am: &mut AudioManager, path: &str) -> Result<(), String> {
    am.load_soundfont(path, true)
}

/// Helper to check if soundfont is active
#[cfg(feature = "soundfont")]
fn is_sf_active(am: &AudioManager) -> bool {
    am.soundfont_state() == SoundfontState::Ready && am.is_using_soundfont()
}
```

### 4.2 Mock Audio Output (for CI)

```rust
// For headless testing without audio hardware
#[cfg(test)]
mod mock_audio {
    // Override OutputStream::try_default() to return virtual device
}
```

---

## 5. Test Execution

### 5.1 Local Testing

```bash
# Run all soundfont tests
cargo test --features "audio,soundfont" soundfont

# Run specific test category
cargo test --features "audio,soundfont" soundfont::resolve_path
cargo test --features "audio,soundfont" soundfont::integration

# Run with output
cargo test --features "audio,soundfont" -- --nocapture soundfont
```

### 5.2 CI/CD Pipeline

```yaml
# .github/workflows/test.yml (example)
jobs:
  test:
    strategy:
      matrix:
        features:
          - "audio"           # MP3 only
          - "audio,soundfont" # With soundfont
    steps:
      - name: Run soundfont tests
        run: cargo test --features ${{ matrix.features }}
```

---

## 6. Logging Verification

### 6.1 Expected Log Messages

| Scenario | Expected Log Level | Message Contains |
|----------|-------------------|------------------|
| SF loaded | `info` | "Soundfont loaded successfully" |
| SF file not found | `error` | "Soundfont file not found" |
| SF parse error | `error` | "Invalid soundfont format" |
| Fallback activated | `info` | "Falling back to MP3" |
| Fallback disabled | `warn` | "Fallback disabled" |
| Runtime toggle | `info` | "Soundfont toggled" |

### 6.2 Log Assertion Pattern

```rust
#[test]
fn test_fallback_logged() {
    let output = capture_log_output(|| {
        // Trigger fallback
        let mut am = AudioManager::new();
        let _ = am.load_soundfont("nonexistent.sf2", true);
    });
    
    assert!(output.contains("fallback"), 
        "Expected fallback log message, got: {}", output);
}
```

---

## 7. Performance Tests

### 7.1 Memory Usage Tests

```rust
#[test]
fn test_memory_with_soundfont() {
    // Measure baseline memory
    let baseline = get_memory_usage();
    
    // Load soundfont
    let mut am = AudioManager::new();
    am.load_soundfont("piano.sf2", true).unwrap();
    
    // Measure after loading
    let after_load = get_memory_usage();
    let increase = after_load - baseline;
    
    // Should be < 50MB for a typical piano SF2
    assert!(increase < 50_000_000, 
        "Memory increase {} exceeds 50MB threshold", increase);
}
```

### 7.2 Latency Tests

```rust
#[test]
fn test_note_latency() {
    // Start timer
    let start = std::time::Instant::now();
    
    // Play note
    am.play_note_by_midi(60);
    
    // Stop timer (when audio callback received)
    let latency = start.elapsed();
    
    // Should be < 50ms
    assert!(latency.as_millis() < 50);
}
```

---

## 8. Migration Tests

### 8.1 Backward Compatibility

| Test | Description | Expected |
|------|-------------|----------|
| MC-01 | Build without `soundfont` feature | MP3 playback works as before |
| MC-02 | Existing `--preload_samples` flag | Works unchanged |
| MC-03 | No soundfont file present | Graceful fallback to MP3 |

### 8.2 Feature Parity

| Feature | MP3 Mode | Soundfont Mode |
|---------|----------|----------------|
| Note playback | ✓ | ✓ |
| Polyphony | Limited by samples | Full polyphony |
| Latency | ~50-100ms | ~5-20ms |
| CPU usage | Low | Medium |

---

## 9. Test Coverage Goals

| Category | Target Coverage |
|----------|-----------------|
| Soundfont manager module | 90% |
| Path resolution | 95% |
| Fallback logic | 100% |
| Error handling | 90% |
| Integration with AudioManager | 85% |

---

## 10. Test Maintenance

### 10.1 When to Update Tests

1. When adding new soundfont features
2. When modifying path resolution logic
3. When changing error handling behavior
4. When platform support changes

### 10.2 Test Review Checklist

- [ ] All new features have test coverage
- [ ] All error paths are tested
- [ ] Logs are verified for critical events
- [ ] Platform-specific tests run on all platforms
- [ ] Performance tests have baseline measurements

---

## 11. Appendix: Test Script Examples

### 11.1 Quick Test Script

```bash
#!/bin/bash
# run_soundfont_tests.sh

set -e

echo "=== Building with soundfont feature ==="
cargo build --features "audio,soundfont"

echo "=== Running unit tests ==="
cargo test --features "audio,soundfont" soundfont::unit

echo "=== Running integration tests ==="
cargo test --features "audio,soundfont" soundfont::integration

echo "=== Running all soundfont tests ==="
cargo test --features "audio,soundfont" soundfont

echo "=== Running without soundfont feature (regression) ==="
cargo test --features "audio"

echo "=== All tests passed! ==="
```

### 11.2 Platform-Specific Test Commands

**Windows (PowerShell):**
```powershell
cargo test --features "audio,soundfont" -- --test-threads=1
```

**macOS/Linux:**
```bash
cargo test --features "audio,soundfont" -- --test-threads=1
```

---

## 12. Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-03-29 | Architecture | Initial test plan |

---

*End of Document*