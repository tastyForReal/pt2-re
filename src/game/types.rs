/// Screen and game configuration constants mirroring the TypeScript SCREEN_CONFIG.
pub const SCREEN_WIDTH: f32 = 360.0;
pub const SCREEN_HEIGHT: f32 = 640.0;
pub const COLUMN_COUNT: u32 = 4;
pub const BASE_ROW_HEIGHT: f32 = SCREEN_HEIGHT / 4.0;
pub const SCROLL_SPEED: f32 = BASE_ROW_HEIGHT * 3.0;
pub const GRID_LINE_WIDTH: f32 = 1.0;
pub const DEFAULT_TPS: f32 = 3.0;

pub const COLUMN_WIDTH: f32 = SCREEN_WIDTH / COLUMN_COUNT as f32;

/// Key-to-lane mapping: D=0, F=1, J=2, K=3
pub fn key_to_lane(key: &winit::keyboard::Key) -> Option<u32> {
    use winit::keyboard::Key::Character;
    match key {
        Character(c) => match c.as_str() {
            "d" | "D" => Some(0),
            "f" | "F" => Some(1),
            "j" | "J" => Some(2),
            "k" | "K" => Some(3),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RowType {
    SingleTileRow = 0,
    DoubleTileRow = 1,
    EmptyRow = 2,
    StartingTileRow = 3,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RowTypeResult {
    #[serde(rename = "type")]
    pub row_type: RowType,
    pub height_multiplier: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl GameColor {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const RED: Self = Self {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const YELLOW: Self = Self {
        r: 255,
        g: 255,
        b: 0,
        a: 255,
    };

    pub fn to_normalized(&self, opacity: f32) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            opacity,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct TileData {
    pub lane_index: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: GameColor,
    pub opacity: f32,
    pub is_pressed: bool,
    pub is_game_over_indicator: bool,
    pub flash_state: bool,
    pub is_holding: bool,
    pub progress: f32,
    pub is_released_early: bool,
    pub completed_at: Option<f64>,
    pub last_note_played_at: Option<f64>,
    pub active_circle_animations: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct RowData {
    pub row_index: usize,
    pub row_type: RowType,
    pub height_multiplier: f32,
    pub y_position: f32,
    pub height: f32,
    pub tiles: Vec<TileData>,
    pub is_completed: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    Paused = 0,
    Resumed = 1,
    TileMisclicked = 2,
    TileFellOffScreen = 3,
    Flashing = 4,
    Cleared = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GameMode {
    OneRound = 0,
    Endless = 1,
    Survival = 2,
}

#[derive(Debug, Clone)]
pub struct EndlessConfig {
    pub mode: GameMode,
    pub fixed_tps_values: Option<Vec<f32>>,
    pub starting_tps: Option<f32>,
    pub acceleration_rate: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct GameOverFlashState {
    pub tile: TileData,
    pub start_time: f64,
    pub flash_count: u32,
    pub is_flashing: bool,
}

#[derive(Debug, Clone)]
pub struct GameOverAnimationState {
    pub start_time: f64,
    pub duration: f64,
    pub start_offset: f32,
    pub target_offset: f32,
    pub is_animating: bool,
}

#[derive(Debug, Clone)]
pub struct NoteIndicatorData {
    pub note_id: i64,
    pub row_index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub time: f64,
    pub time_fraction: Option<f64>,
    pub track_idx: Option<usize>,
    pub midi: Option<u8>,
    pub is_consumed: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MusicMetadata {
    pub id: u32,
    pub tps: f32,
    pub bpm: f64,
    pub base_beats: f64,
    pub start_row_index: usize,
    pub end_row_index: usize,
    pub row_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputType {
    Mouse = 0,
    Keyboard = 1,
}

#[derive(Debug, Clone)]
pub struct RowTiming {
    pub start_time: f64,
    pub mid_time: f64,
    pub end_time: f64,
}

#[derive(Debug, Clone)]
pub struct LevelData {
    pub rows: Vec<RowTypeResult>,
    pub musics: Vec<MusicMetadata>,
    pub base_bpm: f64,
    pub midi_json: Option<super::midi_types::MidiJson>,
}

/// All mutable game data, mirroring TypeScript's GameData interface.
#[derive(Debug, Clone)]
pub struct GameData {
    pub state: GameState,
    pub rows: Vec<RowData>,
    pub total_completed_height: f32,
    pub scroll_offset: f32,
    pub game_over_data: Option<GameOverFlashState>,
    pub game_over_animation: Option<GameOverAnimationState>,
    pub game_won_time: Option<f64>,
    pub last_single_lane: u32,
    pub last_double_lanes: Option<(u32, u32)>,
    pub active_row_index: usize,
    pub completed_rows_count: u32,
    pub current_tps: f32,
    pub current_music_index: usize,
    pub musics_metadata: Vec<MusicMetadata>,
    pub current_midi_time: f64,
    pub midi_loaded: bool,
    pub has_game_started: bool,
    pub note_indicators: Vec<NoteIndicatorData>,
    pub midi_playing: bool,
    pub target_time_for_next_note: f64,
    pub current_dt_press_count: u32,
    pub skipped_midi_notes: Vec<i64>,
    pub level_row_timings: Vec<RowTiming>,
    pub game_mode: GameMode,
    pub endless_config: Option<EndlessConfig>,
    pub loop_count: u32,
    pub current_filename: String,
    pub raw_level_rows: Vec<RowTypeResult>,
    pub loop_0_midi_notes: Vec<Loop0MidiNote>,
}

#[derive(Debug, Clone)]
pub struct Loop0MidiNote {
    pub track_idx: usize,
    pub midi: u8,
    pub original_time: f64,
    pub row_index: usize,
    pub time_fraction: f64,
}
