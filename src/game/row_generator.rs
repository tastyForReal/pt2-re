use super::types::*;
use rand::Rng;

pub const DEFAULT_ROW_COUNT: usize = 100;

const LANE_X_POSITIONS: [f32; 4] = [0.0, COLUMN_WIDTH, COLUMN_WIDTH * 2.0, COLUMN_WIDTH * 3.0];

const ROW_TYPE_WEIGHT_0: f64 = 0.6;
const ROW_TYPE_WEIGHT_1: f64 = 0.25;
const ROW_TYPE_THRESHOLD_1: f64 = ROW_TYPE_WEIGHT_0;
const ROW_TYPE_THRESHOLD_2: f64 = ROW_TYPE_THRESHOLD_1 + ROW_TYPE_WEIGHT_1;

pub fn calculate_lane_x(lane_index: u32) -> f32 {
    LANE_X_POSITIONS
        .get(lane_index as usize)
        .copied()
        .unwrap_or(0.0)
}

pub fn create_tile(
    lane_index: u32,
    y_position: f32,
    height: f32,
    color: GameColor,
    opacity: f32,
) -> TileData {
    TileData {
        lane_index,
        x: calculate_lane_x(lane_index),
        y: y_position,
        width: COLUMN_WIDTH,
        height,
        color,
        opacity,
        is_pressed: false,
        is_game_over_indicator: false,
        flash_state: false,
        is_holding: false,
        progress: 0.0,
        is_released_early: false,
        completed_at: None,
        last_note_played_at: None,
        active_circle_animations: Vec::new(),
    }
}

pub fn create_start_row() -> (RowData, u32) {
    let mut rng = rand::thread_rng();
    let start_y = SCREEN_HEIGHT - BASE_ROW_HEIGHT * 2.0;
    let lane_index: u32 = rng.gen_range(0..4);
    let row = RowData {
        row_index: 0,
        row_type: RowType::StartingTileRow,
        height_multiplier: 1.0,
        y_position: start_y,
        height: BASE_ROW_HEIGHT,
        tiles: vec![create_tile(
            lane_index,
            start_y,
            BASE_ROW_HEIGHT,
            GameColor::BLACK,
            1.0,
        )],
        is_completed: false,
        is_active: true,
    };
    (row, lane_index)
}

fn determine_double_lanes(preceding_row: Option<&RowData>) -> (u32, u32) {
    let mut rng = rand::thread_rng();
    match preceding_row {
        None => {
            if rng.gen_bool(0.5) {
                (0, 2)
            } else {
                (1, 3)
            }
        }
        Some(row) => match row.row_type {
            RowType::SingleTileRow | RowType::StartingTileRow => {
                match row.tiles.first().map(|t| t.lane_index) {
                    Some(lane) if lane == 0 || lane == 2 => (1, 3),
                    Some(_) => (0, 2),
                    None => {
                        if rng.gen_bool(0.5) {
                            (0, 2)
                        } else {
                            (1, 3)
                        }
                    }
                }
            }
            RowType::DoubleTileRow => {
                let occupied: Vec<u32> = row.tiles.iter().map(|t| t.lane_index).collect();
                if occupied.contains(&0) && occupied.contains(&2) {
                    (1, 3)
                } else {
                    (0, 2)
                }
            }
            _ => {
                if rng.gen_bool(0.5) {
                    (0, 2)
                } else {
                    (1, 3)
                }
            }
        },
    }
}

fn get_random_row_type() -> RowType {
    let mut rng = rand::thread_rng();
    let rand: f64 = rng.gen();
    if rand < ROW_TYPE_THRESHOLD_1 {
        RowType::SingleTileRow
    } else if rand < ROW_TYPE_THRESHOLD_2 {
        RowType::DoubleTileRow
    } else {
        RowType::EmptyRow
    }
}

pub fn generate_all_rows(row_count: usize) -> Vec<RowData> {
    let mut rng = rand::thread_rng();
    let mut rows = Vec::new();
    let (start_row, start_lane) = create_start_row();
    let mut last_single_lane = start_lane;
    let mut current_y = start_row.y_position;
    let mut preceding_row = start_row.clone();
    rows.push(start_row);

    for i in 1..=row_count {
        let height_multiplier = 1 + (rng.gen::<f32>() * 8.0) as u32;
        let row_height = height_multiplier as f32 * BASE_ROW_HEIGHT;
        current_y -= row_height;

        let row_type = get_random_row_type();
        let row = match row_type {
            RowType::SingleTileRow => {
                let lane = pick_single_lane(&preceding_row, last_single_lane);
                last_single_lane = lane;
                RowData {
                    row_index: i,
                    row_type: RowType::SingleTileRow,
                    height_multiplier: height_multiplier as f32,
                    y_position: current_y,
                    height: row_height,
                    tiles: vec![create_tile(
                        lane,
                        current_y,
                        row_height,
                        GameColor::BLACK,
                        1.0,
                    )],
                    is_completed: false,
                    is_active: false,
                }
            }
            RowType::DoubleTileRow => {
                let (l1, l2) = determine_double_lanes(Some(&preceding_row));
                RowData {
                    row_index: i,
                    row_type: RowType::DoubleTileRow,
                    height_multiplier: height_multiplier as f32,
                    y_position: current_y,
                    height: row_height,
                    tiles: vec![
                        create_tile(l1, current_y, row_height, GameColor::BLACK, 1.0),
                        create_tile(l2, current_y, row_height, GameColor::BLACK, 1.0),
                    ],
                    is_completed: false,
                    is_active: false,
                }
            }
            _ => RowData {
                row_index: i,
                row_type: RowType::EmptyRow,
                height_multiplier: height_multiplier as f32,
                y_position: current_y,
                height: row_height,
                tiles: vec![],
                is_completed: true,
                is_active: false,
            },
        };
        preceding_row = row.clone();
        rows.push(row);
    }
    rows
}

fn pick_single_lane(preceding_row: &RowData, last_single_lane: u32) -> u32 {
    let mut rng = rand::thread_rng();
    if preceding_row.row_type == RowType::DoubleTileRow {
        let occupied: Vec<u32> = preceding_row.tiles.iter().map(|t| t.lane_index).collect();
        let empty: Vec<u32> = (0..4).filter(|s| !occupied.contains(s)).collect();
        if empty.is_empty() {
            0
        } else {
            empty[rng.gen_range(0..empty.len())]
        }
    } else {
        let available: Vec<u32> = (0..4).filter(|&s| s != last_single_lane).collect();
        if available.is_empty() {
            0
        } else {
            available[rng.gen_range(0..available.len())]
        }
    }
}

pub fn is_row_visible(row: &RowData, scroll_offset: f32) -> bool {
    let screen_y = row.y_position + scroll_offset;
    screen_y + row.height > 0.0 && screen_y < SCREEN_HEIGHT
}

pub fn generate_rows_from_level_data(level_rows: &[RowTypeResult]) -> Vec<RowData> {
    let mut rng = rand::thread_rng();
    let mut rows = Vec::new();
    let start_y = SCREEN_HEIGHT - BASE_ROW_HEIGHT * 2.0;
    let start_lane: u32 = rng.gen_range(0..4);
    let start_tile = create_tile(start_lane, start_y, BASE_ROW_HEIGHT, GameColor::YELLOW, 1.0);

    rows.push(RowData {
        row_index: 0,
        row_type: RowType::StartingTileRow,
        height_multiplier: 1.0,
        y_position: start_y,
        height: BASE_ROW_HEIGHT,
        tiles: vec![start_tile],
        is_completed: false,
        is_active: true,
    });

    let mut current_y = start_y;
    let mut last_single_lane = start_lane;

    for (i, row_data) in level_rows.iter().enumerate() {
        let row_height = row_data.height_multiplier * BASE_ROW_HEIGHT;
        current_y -= row_height;
        let row_index = i + 1;
        let preceding_row = rows.last();

        let tiles = match row_data.row_type {
            RowType::SingleTileRow => {
                let lane = pick_single_lane_from_opt(preceding_row, last_single_lane);
                last_single_lane = lane;
                vec![create_tile(
                    lane,
                    current_y,
                    row_height,
                    GameColor::BLACK,
                    1.0,
                )]
            }
            RowType::DoubleTileRow => {
                let (l1, l2) = determine_double_lanes(preceding_row);
                vec![
                    create_tile(l1, current_y, row_height, GameColor::BLACK, 1.0),
                    create_tile(l2, current_y, row_height, GameColor::BLACK, 1.0),
                ]
            }
            _ => vec![],
        };

        rows.push(RowData {
            row_index,
            row_type: row_data.row_type,
            height_multiplier: row_data.height_multiplier,
            y_position: current_y,
            height: row_height,
            tiles,
            is_completed: row_data.row_type == RowType::EmptyRow,
            is_active: false,
        });
    }
    rows
}

fn pick_single_lane_from_opt(preceding_row: Option<&RowData>, last_single_lane: u32) -> u32 {
    let mut rng = rand::thread_rng();
    match preceding_row {
        Some(row) if row.row_type == RowType::DoubleTileRow => {
            let occupied: Vec<u32> = row.tiles.iter().map(|t| t.lane_index).collect();
            let empty: Vec<u32> = (0..4).filter(|s| !occupied.contains(s)).collect();
            if empty.is_empty() {
                0
            } else {
                empty[rng.gen_range(0..empty.len())]
            }
        }
        _ => {
            let available: Vec<u32> = (0..4).filter(|&s| s != last_single_lane).collect();
            if available.is_empty() {
                0
            } else {
                available[rng.gen_range(0..available.len())]
            }
        }
    }
}
