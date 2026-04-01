use super::audio_manager::AudioManager;
use super::game_state;
use super::input_handler::InputHandler;
use super::json_level_reader;
use super::note_indicator;
use super::row_generator;
use super::score_manager::ScoreManager;
use super::types::*;
use std::time::Instant;

/// Top-level game controller that orchestrates game state, input, and rendering triggers.
pub struct GameController {
    pub game_data: GameData,
    pub score_manager: ScoreManager,
    pub audio_manager: AudioManager,
    pub input_handler: InputHandler,
    pub is_paused: bool,
    pub enable_autoplay: bool,
    start_time: Option<Instant>,
    last_update_time: Option<Instant>,
}

impl GameController {
    pub fn new(enable_autoplay: bool) -> Self {
        // Default random game
        let rows = row_generator::generate_all_rows(row_generator::DEFAULT_ROW_COUNT);
        let game_data = game_state::create_game_data(
            rows,
            Vec::new(),
            GameMode::OneRound,
            None,
            Vec::new(),
            String::new(),
        );
        Self {
            game_data,
            score_manager: ScoreManager::new(),
            audio_manager: crate::game::audio_manager::AudioManager::new(),
            input_handler: InputHandler::new(),
            is_paused: false,
            enable_autoplay,
            start_time: None,
            last_update_time: None,
        }
    }
    /// Load a level from a JSON file path.
    pub fn load_level_from_file(&mut self, path: &str) -> Result<(), String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file {}: {}", path, e))?;
        let json: serde_json::Value =
            serde_json::from_str(&contents).map_err(|e| format!("Invalid JSON: {}", e))?;
        let level_data = json_level_reader::load_level_from_json(&json)?;

        let filename = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();

        self.load_level(level_data, GameMode::OneRound, None, filename);
        Ok(())
    }
    pub fn load_level(
        &mut self,
        level_data: LevelData,
        game_mode: GameMode,
        endless_config: Option<EndlessConfig>,
        filename: String,
    ) {
        let rows = row_generator::generate_rows_from_level_data(&level_data.rows);
        let raw_rows = level_data.rows.clone();

        let mut game_data = game_state::create_game_data(
            rows,
            level_data.musics.clone(),
            game_mode,
            endless_config,
            raw_rows,
            filename,
        );

        // Build note indicators if MIDI data is available
        if let Some(ref midi) = level_data.midi_json {
            game_data.note_indicators =
                note_indicator::build_note_indicators(midi, &game_data.rows, &level_data.musics);
            game_data.midi_loaded = true;

            let mut all_loop_notes = Vec::new();
            for (track_idx, track) in midi.tracks.iter().enumerate() {
                for note in &track.notes {
                    let mut target_row_index: isize = -1;
                    let mut target_fraction: f64 = 0.0;
                    for (r, timing) in game_data.level_row_timings.iter().enumerate() {
                        if note.time >= timing.start_time
                            && note.time <= timing.end_time
                            && timing.end_time > timing.start_time
                        {
                            target_row_index = (r + 1) as isize;
                            target_fraction = (note.time - timing.start_time)
                                / (timing.end_time - timing.start_time);
                            break;
                        }
                    }
                    if target_row_index != -1 {
                        all_loop_notes.push(Loop0MidiNote {
                            track_idx,
                            midi: note.midi,
                            original_time: note.time,
                            row_index: target_row_index as usize,
                            time_fraction: target_fraction,
                        });
                    }
                }
            }
            game_data.loop_0_midi_notes = all_loop_notes;

            // Load MIDI data into AudioManager!
            self.audio_manager.load_midi_data(midi.clone());
        }

        self.game_data = game_data;
        self.score_manager.reset();
        self.audio_manager.reset_playback();
        self.start_time = None;
        self.last_update_time = None;
        self.is_paused = false;
    }

    /// Reset to a random game.
    pub fn reset_random(&mut self) {
        let rows = row_generator::generate_all_rows(row_generator::DEFAULT_ROW_COUNT);
        self.game_data = game_state::create_game_data(
            rows,
            Vec::new(),
            GameMode::OneRound,
            None,
            Vec::new(),
            String::new(),
        );
        self.score_manager.reset();
        self.audio_manager.clear_midi_data();
        self.start_time = None;
        self.last_update_time = None;
        self.is_paused = false;
    }

    /// Called each frame to advance game logic.
    pub fn update(&mut self) {
        let now = Instant::now();
        if self.start_time.is_none() {
            self.start_time = Some(now);
        }
        let current_time = now.duration_since(self.start_time.unwrap()).as_secs_f64() * 1000.0;

        let dt = match self.last_update_time {
            Some(last) => now.duration_since(last).as_secs_f64(),
            None => 0.0,
        };
        self.last_update_time = Some(now);

        if self.is_paused {
            return;
        }

        // Autoplay bot
        if self.enable_autoplay {
            game_state::update_bot(
                &mut self.game_data,
                &mut self.audio_manager,
                &mut self.score_manager,
                current_time,
            );
        }

        game_state::update_scroll(
            &mut self.game_data,
            dt,
            current_time,
            &mut self.audio_manager,
            &mut self.score_manager,
        );
        game_state::update_game_over_flash(&mut self.game_data, current_time);
        game_state::update_game_over_animation(&mut self.game_data, current_time);
        game_state::update_game_won(&mut self.game_data, current_time);
        self.score_manager.update(current_time);
    }

    /// Handle key press/release for game lanes.
    pub fn handle_key_input(&mut self, event: &winit::event::KeyEvent) {
        let current_time = self.current_time_ms();
        if let Some((lane, is_press)) = self.input_handler.handle_key_event(event) {
            if is_press {
                game_state::handle_tile_press(
                    &mut self.game_data,
                    lane,
                    None,
                    InputType::Keyboard,
                    current_time,
                    &mut self.audio_manager,
                    &mut self.score_manager,
                    self.enable_autoplay,
                );
            } else {
                game_state::handle_tile_release(
                    &mut self.game_data,
                    lane,
                    current_time,
                    &mut self.audio_manager,
                    &mut self.score_manager,
                    self.enable_autoplay,
                );
            }
        }
    }

    /// Handle mouse click for game lanes.
    pub fn handle_mouse_input(
        &mut self,
        state: winit::event::ElementState,
        button: winit::event::MouseButton,
    ) {
        let current_time = self.current_time_ms();
        if let Some((lane, is_press)) = self.input_handler.handle_mouse_button(state, button) {
            let mouse_y = self.input_handler.mouse_y();
            if is_press {
                game_state::handle_tile_press(
                    &mut self.game_data,
                    lane,
                    Some(mouse_y),
                    InputType::Mouse,
                    current_time,
                    &mut self.audio_manager,
                    &mut self.score_manager,
                    self.enable_autoplay,
                );
            } else {
                game_state::handle_tile_release(
                    &mut self.game_data,
                    lane,
                    current_time,
                    &mut self.audio_manager,
                    &mut self.score_manager,
                    self.enable_autoplay,
                );
            }
        }
    }

    pub fn handle_cursor_moved(&mut self, x: f32, y: f32) {
        self.input_handler.update_cursor_position(x, y);
    }

    pub fn toggle_pause(&mut self) {
        if !self.game_data.has_game_started {
            // Cannot toggle resume if not started.
            self.is_paused = true;
            self.game_data.state = GameState::Paused;
            return;
        }
        self.is_paused = !self.is_paused;
        if self.is_paused {
            self.game_data.state = GameState::Paused;
        } else {
            self.game_data.state = GameState::Resumed;
            // Reset timing to avoid jump
            self.last_update_time = Some(Instant::now());
        }
        log::info!(
            "Game {} ",
            if self.is_paused { "paused" } else { "resumed" }
        );
    }

    pub fn pause(&mut self) {
        if !self.is_paused {
            self.toggle_pause();
        }
    }

    /// Handle window focus lost — pause.
    pub fn handle_focus_lost(&mut self) {
        if !self.is_paused && self.game_data.has_game_started {
            self.pause();
        }
    }

    pub fn get_current_time_ms(&self) -> f64 {
        self.current_time_ms()
    }

    fn current_time_ms(&self) -> f64 {
        match self.start_time {
            Some(start) => Instant::now().duration_since(start).as_secs_f64() * 1000.0,
            None => 0.0,
        }
    }

    pub fn has_game_started(&self) -> bool {
        self.game_data.has_game_started
    }

    pub fn is_game_over(&self) -> bool {
        matches!(
            self.game_data.state,
            GameState::Flashing | GameState::TileMisclicked | GameState::TileFellOffScreen
        )
    }

    pub fn is_game_won(&self) -> bool {
        self.game_data.state == GameState::Cleared
    }
}
