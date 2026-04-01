# Piano Tiles Re:U: TypeScript to Rust Port Worklog

## Overview

This document outlines the process, challenges, and methodologies used in porting the "Piano Tiles Re:U" from a TypeScript/WebGPU codebase to a native Rust application utilizing `wgpu`.

## Implementation Process

### 1. Environment and Project Initialization

- Verified the presence of the Rust toolchain (`rustc`, `cargo`).
- Configured a new Cargo workspace and initialized the `pt2` binary.
- Mirrored the existing project structure from the TypeScript repository (e.g., `src/game/`, `src/renderer/`, `assets/`, `shaders/`).
- Created a `Cargo.toml` with necessary dependencies, most notably `wgpu` for cross-platform GPU access, `winit` for windowing/input, `serde` for JSON data structures, and `rodio` for audio (which was feature-gated).

### 2. Core Data Structures and Game Logic

- Translated basic types (e.g., `TileData`, `RowData`, `GameState`) from TypeScript `interface`s into Rust `struct`s with precise types (e.g., strong `enum`s for `RowType` and `GameState`).
- Maintained JSON backward compatibility with original levels by utilizing `serde` aliases (`#[serde(rename = "baseBeats")]`).
- Replicated the core game state loop. This included logic for updating tile offsets via `scroll_offset`, advancing active rows, checking bounds for missed tiles, and tracking score.
- Ported the randomized `row_generator` mapping `Math.random()` patterns to stable Rust equivalents using the `rand` crate.

### 3. Engine Translation (WebGPU to wgpu)

- **Shaders**: Extracted the inline WGSL shaders from TypeScript strings into standalone `shaders/*.wgsl` files.
- **Windowing**: Replaced browser DOM events with a `winit` event loop implementing `ApplicationHandler`. Added customized focus management (auto-pausing the game if focus is lost) and mapped keys strictly to the D, F, J, K layout.
- **Sub-renderers**: Adapted the `TileRenderer`, `SpriteRenderer`, `ScoreRenderer`, and `BitmapFontRenderer` architectures into safe Rust patterns. Replaced JS object mapping bounds with wgpu buffers and bind groups.
- **Font Parsing**: Switched the `BitmapFontParser` to manual string parsing matching the `.fnt` standard to avoid heavyweight dependencies, resolving `xadvance` layouts properly.
- **Sprite Parsing**: Handled `.plist` reading logic using `roxmltree` and regexes (`regex-lite`) instead of HTML `DOMParser`.

### 4. Level & MIDI Parsers

- Translated the somewhat complex custom JSON level loader and embedded string-based music parser into strong Rust states.
- Ensured timing structures computed via beat division (like `256`, `128`) match up precisely to `f64` precise `std::time::Instant` usage in the main controller loop, avoiding reliance on browser `performance.now()`.

### 5. CLI & Audio Feature

- Implemented a simple CLI using `clap`: includes a `--file` argument to load JSON levels immediately and `--autoplay` for testing.
- Wrapped audio interactions behind a `#[cfg(feature = "audio")]` flag matching standard Rust compilation practices.

### 6. Parity Synchronization & Input Refinement

- **Font Rendering Fix**: Resolved an issue where text characters appeared as solid rectangles by updating `shaders/font.wgsl` to utilize the alpha channel (`tex_color.a`) as the mask instead of the red channel.
- **MIDI Synchronization**: Implemented `verify_track_length`, `shrink_track`, and `align_tracks_across_parts` in `json_level_reader.rs` to ensure all MIDI tracks within a song part have identical durations, matching the TS implementation's strict timing.
- **Input Precision**: Refined `handle_tile_press` with 1:1 parity logic:
  - Implemented `timing_zone` (bottom 50% of screen) for keyboard input.
  - Added Y-coordinate validation and `hit_zone` checks (bottom segment only) for mouse/touch interactions on long tiles.
  - Corrected the starting tile's interaction logic to allow transitions from `Paused` to `Resumed` states.
- **State Machine Enhancements**:
  - Added `trigger_game_won` detection.
  - Implemented auto-skipping for empty rows with integrated MIDI stopwatch updates.
  - Added `skip_notes_for_active_row` to handle early release of long tiles, preventing delayed note triggers.
- **Autoplay Bot**: Synchronized the bot's execution with the MIDI stopwatch, ensuring accurate playback even during high-speed or complex sequences.
- **Color Space Parity**: Resolved a visual discrepancy where colors appeared washed out compared to the TS version.
  - Switched surface format selection to prefer non-sRGB formats (e.g., `Bgra8Unorm`), matching browser `getPreferredCanvasFormat()` behavior.
  - Updated `SpriteRenderer` and `BitmapFontRenderer` to utilize `Rgba8Unorm` texture formats, bypassing automatic linear-to-sRGB conversion.
- **wgpu Maintenance**: Updated deprecated types (`ImageCopyTexture`, `ImageDataLayout`) to their v24 equivalents (`TexelCopyTextureInfo`, `TexelCopyBufferLayout`) in `sprite_renderer.rs`.
- **Game Over & Won Synchronization**: Achieved 1:1 behavioral parity for end-game states:
  - Implemented `trigger_game_over_out_of_bounds` with repositioning animations and unpressed tile flashing logic.
  - Refactored the main update loop into specialized sub-functions: `update_game_over_flash`, `update_game_over_animation`, and `update_game_won`.
  - Allowed the 'R' key to restart the level from any state.
  - Refined input handling during autoplay to trigger game over on misclicks while ignoring valid tiles, matching TS behavior.
  - Removed redundant "PAUSED", "GAME OVER", and "CLEARED" text labels from the renderer.
  - Updated the window title dynamically to `{file name} - Piano Tiles Re:U`.
- **Input & Animation Refinements**:
  - Fixed the `P` key resume logic to prevent the game from starting before the starting tile is clicked.
  - Fixed the starting tile's click animation by correctly tracking its completion time (`completed_at`).
  - Synchronized `render_short_tile` with the TS version's frame-based press animation (`1.png`, `2.png`, etc.).
  - **Ghosting Bugfix**: Resolved a "double-drawing" issue where tiles would stay visible during game-over flashing. Synchronized `flash_state` from `game_over_data` into the main sprite pass to ensure sprites and primitive colors hide/show in perfect unison.
- **Audio Playback Implementation**:
  - Enabled the `audio` feature-gate and integrated the `rodio` crate for native cross-platform audio playback.
  - Implemented full asset loading for piano samples (MP3) from the local filesystem into an in-memory buffer cache.
  - Mapped MIDI note numbers to sample filenames (e.g., 60 to `c1.mp3`) using the existing `midi_to_note` utility.
  - Synchronized MIDI audio playback with the game's stopwatch, triggering precise sample launches within the `update_midi_playback` loop.
  - Implemented sound-group management using `rodio::Sink` to allow parallel playback with a global `stop_all_samples` capability for game-over/reset scenarios.
  - Added audio feedback for 1:1 parity: "Game Over" chord playback on misclicks/out-of-bounds, and random sample triggering for levels without MIDI data.

### 7. Parity Refinement (MIDI & JSON)

- **MIDI Parser State Machine**: Updated `parse_track_score` to strictly match the TypeScript implementation's state machine, adding missing mode transitions and error logging for invalid sequences (e.g., missing `[` before length).
- **MIDI Processing Parity**: Refined `process_notes` in `json_level_reader.rs` to utilize a more robust note-splitting loop mirroring the TS version's boundary handling.
- **Track Shrinking Fix**: Corrected `shrink_track` to reset message values to zero when consuming notes for alignment, preventing ghost note triggers.
- **Validation Synchronization**: Added final state verification to MIDI score parsing to detect incomplete token sequences.
- **Stopwatch & Bot Logic Sync**:
  - Fixed `update_bot` to trigger MIDI updates for each tile hit/held, ensuring `DoubleTileRow` reach `end_time` correctly.
  - Corrected `update_active_row` to only reset `current_dt_press_count` when the active row changes, preventing premature resets during double tile interactions.
  - Synchronized stopwatch jump logging with the TypeScript implementation (silenced to DEBUG level to reduce console noise during normal play).

### 8. Code Quality and Linting

- **Automated Formatting**: Applied `cargo fmt --all` across the entire workspace to ensure consistent code style and readability.
- **Static Analysis**: Resolved all `cargo clippy` warnings (treated as errors) to enforce Rust best practices:
  - Refactored nested `if` blocks into consolidated logical conditions (`collapsible_if`).
  - Optimized loops by replacing range indexing (`0..len()`) with more efficient iterators and `enumerate()`.
  - Simplified code using modern Rust features like `is_some_and`, `is_multiple_of`, and `RangeInclusive::contains`.
  - Improved performance by utilizing `to_vec()` for slice conversions and passing slices (`&mut [_]`) instead of `&mut Vec`.
  - Optimized iterator chains by replacing `.filter().last()` with `.rfind()`.

### 9. TPS Initialization Fix

- **Initial TPS from Level Data**: Fixed a bug where `current_tps` was always initialized to `DEFAULT_TPS` (3.0) in `create_game_data`, ignoring the level's actual BPM-derived TPS from the first music section.
  - Updated `create_game_data` in `game_state.rs` to compute `initial_tps` from `musics_metadata[0].tps`, falling back to `DEFAULT_TPS` only when no music sections exist.
  - Added Survival mode handling: uses `endless_config.starting_tps` when available, matching the TypeScript `load_level()` logic at 1:1 parity.
  - Added `log::info!` calls to print the resolved initial TPS on level load for easier debugging.

### 10. CLI Gameplay Mode & Speed Customization

- **Enhanced CLI Options**: Extended the `clap`-based CLI with advanced gameplay configuration arguments mirroring the TypeScript version's `CustomizeDialog`:
  - `--gamemode`: Supports `one_round`, `endless`, and `survival` modes.
  - **Pacing Overrides**: Added support for per-section TPS (`--starting_tpses`) or BPM (`--starting_bpms`) overrides for `one_round` and `endless` modes.
  - **Survival Configuration**: Added single-value `--starting_tps` / `--starting_bpm` and a configurable `--acceleration_rate` (defaulting to `0.01` TPS/sec).
- **Parity Logic Implementation**:
  - Ported the BPM-to-TPS conversion logic using each music section's `base_beats`.
  - Ensured that the application respects level defaults (`baseBpm`/`bpm`) when no explicit overrides are provided via CLI.
  - Synchronized the initialization flow in `main.rs` to construct the appropriate `GameMode` and `EndlessConfig` structs before level loading, achieving 1:1 behavioral parity with the web-based dialog.

### 11. Endless & Survival Gameplay Parity

- **Survival Mechanics**: Verified and completed the survival pace behavior where TPS accelerates by a constant rate in `update_challenge_tps` using `delta_time` in seconds. Modified the renderer pipeline data to override the standard scoring display with `current_tps.toFixed(3)` behavior.
- **Endless Expansion Logic**: Assured accuracy on the recursive row generator adding `+0.333 TPS` to every subsequent loop transition in `GameMode::Endless`.
- **Game Won Condition Fix**: Guarded the level-clear win conditions within `game_state.rs` strictly to `GameMode::OneRound`, properly matching TS expectations where Endless/Survival modes continue until failure.
- **Invisible Track Audio Fix**: Resolved an issue where MIDI playback (specifically background backing tracks or invisible arpeggio chords) would randomly stop on subsequent laps. The system originally fed the `AudioManager` by iterating over `lp0_indicators`, inadvertently skipping notes without visual hit indicators. Populated `loop_0_midi_notes` linearly and synced loop processing natively to feed _all_ original map coordinates to the audio engine accurately.
- **Manual Loop Triggering Fix**: Fixed a bug where Endless/Survival modes would stop generating rows after the first lap if the user wasn't using `--autoplay`. Extracted the music section tracking into `check_and_update_music_for_row` and moved it into the main `update_scroll` loop so that map regeneration triggers reliably by monitoring the active row's progress through the level parts.

### 12. Responsive Window Resizing & Aspect Preservation

- **Dynamic Scaling Architecture**: Enabled window resizing via `winit` and implemented a flexible rendering pipeline that handles arbitrary resolutions:
  - **Height-Responsive Layout**: Updated all renderer components (`TileRenderer`, `SpriteRenderer`, `BitmapFontRenderer`) to use `GpuContext`'s actual window dimensions. Elements now scale their height dynamically based on `actual_h / SCREEN_HEIGHT`, ensuring consistent vertical density across all window sizes.
  - **Selective Width Stretching**: Implemented specialized scaling logic for standard tiles:
    - `tile_black.png` and `tile_start.png` (including transition frames `1.png`-`4.png`) stretch to fill the window width, maintaining a "full-bleed" lane appearance.
    - All other elements (long tiles, progress indicators, arpeggio dots) preserve their original width and are centered horizontally using an `offset_x` calculated from the remaining letterbox space.
  - **Unified Coordinate Mapping**: Synchronized the `InputHandler` cursor mapping with the new centered coordinate system. Mouse and touch interactions now correctly align with visible tiles regardless of window aspect ratio or scaling.
  - Updated vertical grid lines and the score/status HUD to utilize the same centering logic, providing a stable visual frame for the centered lanes. Corrected the `START` text rendering to match the width-stretching behavior of its parent tile.

### 13. Sprite Aspect Ratio & Text Centering

- **Aspect Ratio Preservation**: Updated rendering logic for "long_light.png", "long_tap2.png", "long_tilelight.png", "dot.png", "dot_light.png", "circle_light.png", and "long_head.png" (and "long_finish.png" for consistency).
  - Switched from `scale_w` to `scale_h` for width calculations to preserve the original sprite aspect ratios regardless of window width.
  - Implemented manual lane-centering by calculating the stretched lane centers and subtracting half of the aspect-ratio-preserved width.
- **START Text Alignment**: Refined the starting tile's "START" text rendering:
  - Switched font scaling to use `scale_h` to prevent characters from appearing stretched in wide windows.
  - Recalculated horizontal position to precisely center the non-stretched text within the width-stretched starting tile.

### 14. Responsive Sprite Scaling Refinement

- **Differentiated Scaling Model**: Refined sprite rendering logic to distinguish between element types during window resizing:
  - **Group 1 (Fill Width, Preserve Aspect Ratio)**: Implemented lane-filling width (`scale_w`) while preserving height aspect ratio for "long_light.png", "long_tap2.png", "long_tilelight.png", and "long_head.png". 
    - Switched fixed-height calculations for `long_head.png` and `long_light.png` from `scale_h` to `scale_w` to ensure they maintain their original proportions relative to the lane width.
    - Scaled `insets` margins by `scale_w` to preserve the visual appearance of rounded corners and glow effects regardless of window width.
    - Adjusted the `scissor` rectangle for "long_light.png" to start at the top of the screen (`0.0`) and extend to the bottom of the tile (`rect_y + draw_h`), allowing it to go past the tile's top edge while still being trimmed at the screen's top.
  - **Group 2 (Full Stretch)**: Maintained bidirectional stretching (`scale_w` for width and `scale_h` for height) for "long_finish.png", "tile_black.png", "tile_start.png", and the short-tile press animation sequence ("1.png" through "4.png"). This ensures they fill the target area completely, matching the "full-bleed" lane appearance.

  ## Key Challenges and Solutions

1.  **Borrow Checker Constraints in State Tree**
    - _Challenge_: The TypeScript controller mutated deeply embedded game state references on the fly (e.g., modifying score while tracking row completion concurrently). This caused multiple mutable vs non-mutable borrow-checking errors.
    - _Solution_: Disentangled overlapping lifecycle requests by utilizing localized arrays and deferred modifications. E.g., cloning targeted `TileData` structs into an ephemeral vector before feeding them into the score mutation system.

2.  **WGSL and Asset Binding**
    - _Challenge_: Porting the WebGPU buffer management meant handling low-level byte casting which can be tricky when data alignment shifts between environments.
    - _Solution_: Extensively utilized `bytemuck::Pod` + `Zeroable` traits to automatically enforce strict runtime memory assertions against unaligned shader buffer injections.

3.  **Parsing XML & Regex logic**
    - _Challenge_: The TypeScript source heavily relied on browser APIs like `DOMParser` for `.plist` processing which doesn't directly exist in native Rust contexts.
    - _Solution_: Switched into statically-typed XML traversals using lightweight deps like `roxmltree` and `regex-lite`, providing zero-compromise matching directly equivalent to the browser implementations but without heavy cross-dependency footprints.

4.  **Deep Dive Structural Alignment**
    - _Challenge_: The TypeScript game state handled some unique edge cases for procedural loop generation (TPS scale modifiers, time-fraction predictions for MIDI mappings across level barriers, and custom `SafeDivider` arpeggio processing logic inside string representations) which were initially simplified in the Rust port.
    - _Solution_: Executed a 1-to-1 reconstruction of `game_state.rs`, restructuring track sequences on the fly using cloned configurations scaling to infinite row buffers. Ported the `process_notes` notation loops with strict match constraints (`@`, `%`, `!`, `^`, `&`) mapped directly opposite to the TS iteration bounds logic to achieve pure feature parity.
5.  **Input and Timing Parity**
    - _Challenge_: User input felt "mushy" or inconsistent compared to the TS version, especially regarding when a press was registered relative to the tile's screen position.
    - _Solution_: Replicated the TS "timing zone" and "hit zone" logic exactly. Keyboard inputs are now gated by screen height, and mouse clicks on long tiles are restricted to the base hit zone. This restored the precision feel of the original game.

6.  **Bitmap Font Transparency**
    - _Challenge_: Text characters were rendering as solid blocks, obscuring the game background.
    - _Solution_: Discovered the font texture was utilizing the alpha channel for transparency rather than the red channel. Updated the fragment shader to sample `tex_color.a`, resulting in crisp, transparent text rendering.
7.  **Color Space Inconsistency (WebGPU vs wgpu)**
    - _Challenge_: Colors in the Rust port appeared significantly brighter and "washed out" compared to the original TypeScript/WebGPU version.
    - _Solution_: Identified that `wgpu` was defaulting to sRGB surface and texture formats, which triggers automatic gamma correction. Switched to `Unorm` (linear) formats for both the surface and asset textures (sprites/fonts) to bypass this correction, matching the browser's raw byte handling and restoring 1:1 color parity.

8.  **Audio Playback CPU Overhead**
    - _Challenge_: Rapid audio sample playback (e.g., during arpeggios or multi-note sequences) caused significant CPU spikes and memory allocation churn.
    - _Solution_: Optimized the `AudioManager` with two key architectural changes:
      - **Zero-Copy Playback**: Switched from `Vec<f32>` to `Arc<[f32]>` for stored audio samples and implemented a custom `SharedSource` to stream data directly from memory without per-note allocations or cloning.
      - **Weighted Mixer**: Replaced the "new Sink per note" pattern (which involved heavy audio graph node creation/destruction) with a persistent `DynamicMixer`. All samples are now pushed to a single long-lived mixer source, significantly reducing the overhead of managing concurrent audio streams.
      - **Infinite Mixer Bugfix**: Resolved an issue where rodio's `DynamicMixer` would prematurely stop playback if there were no initial active sources. Wrapped the mixer in an `InfiniteMixerSource` to continuously feed silence, keeping the output sink alive and ready to mix newly triggered sources synchronously with gameplay.
      - **Parallel Startup Decoding**: Replaced sequential mp3 decoding on application boot with `std::thread::scope` based parallelism, dramatically reducing startup times from 14+ seconds down to ~2 seconds by decoding all 91 samples concurrently.
      - **Lazy Background Caching**: Implemented a `--preload-samples` toggle. When left disabled (the default setting), the application launches instantaneously (~0s) utilizing only `~40MB` of RAM. It handles audio via "Lazy Caching"—when a note is played for the first time, a detached `std::thread::spawn` background thread securely decodes the MP3 file without blocking the game's event loop, seamlessly injecting the zero-copy array into an `Arc<RwLock>` cache for instant stutter-free playback on all future presses.

### 15. Sprite Rendering Fixes

- **Sprite Clipping Adjustment**: Verified and applied the scissor fix for `long_light.png` in `renderer.rs`. Adjusted the scissor rectangle to start at screen top (`0.0`) and extend to the tile's bottom edge (`rect_y + draw_h`). This allows the light effect to extend above the tile's visual boundary (e.g., when the tile is near the top of the screen) while still being clipped correctly at the bottom.

### 16. Dynamic Sprite Sizing

- **Refactoring Hardcoded Values**: Replaced hardcoded sprite dimensions (e.g., `134.0`, `324.0`) in `renderer.rs` with dynamic lookups from the parsed `.plist` data.
- **Sprite Size Cache**: Implemented a `HashMap<String, (f32, f32)>` in `Renderer` to cache source dimensions of all loaded sprites. This replaces the rigid `SpriteMetrics` struct, decoupling the renderer from specific field names and allowing for flexible, data-driven sprite lookups.
- **Spritesheet Parser Update**: Added `get_sprite_size` to `SpritesheetData` to expose the source dimensions of loaded sprites.

## Future Recommendations

- Refactor the `AudioManager` to support full sample-based playback and dynamic volume scaling.
- Extend the `Renderer` to support additive blending modes for hit effects and circular animations.
- Implement a more robust "Highscore" persistence layer using local file storage.
