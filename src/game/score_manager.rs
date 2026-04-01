use super::score_types::*;
use super::types::*;

pub struct ScoreManager {
    score_data: ScoreData,
}

impl ScoreManager {
    pub fn new() -> Self {
        Self {
            score_data: ScoreData::default(),
        }
    }

    pub fn get_score_data(&self) -> &ScoreData {
        &self.score_data
    }

    pub fn get_total_score(&self) -> u32 {
        self.score_data.total_score
    }

    pub fn reset(&mut self) {
        self.score_data = ScoreData::default();
    }

    pub fn add_tile_score(&mut self, tile: &TileData, row: &RowData, current_time: f64) -> u32 {
        let score = calculate_tile_score(tile, row);
        if score > 0 {
            self.score_data.total_score += score;
            trigger_score_animation(&mut self.score_data, current_time);
            if row.height_multiplier > 1.0 && !tile.is_released_early {
                let label = create_bonus_label(tile, score, current_time);
                self.score_data.bonus_labels.push(label);
            }
        }
        score
    }

    pub fn update(&mut self, current_time: f64) {
        update_score_animation(&mut self.score_data, current_time);
        update_bonus_label_animations(&mut self.score_data, current_time);
    }
}

fn calculate_tile_score(tile: &TileData, row: &RowData) -> u32 {
    if row.row_type == RowType::EmptyRow {
        return 0;
    }
    if (row.height_multiplier - 1.0).abs() < 0.01 {
        return match row.row_type {
            RowType::SingleTileRow | RowType::StartingTileRow => 1,
            RowType::DoubleTileRow => 2,
            _ => 0,
        };
    }
    let hm = row.height_multiplier as u32;
    if tile.is_released_early && tile.progress < tile.height {
        return (tile.progress / tile.height * hm as f32) as u32 + 1;
    }
    hm + 1
}

fn trigger_score_animation(score: &mut ScoreData, current_time: f64) {
    score.animation = ScoreAnimationState {
        current_scale: 1.0,
        target_scale: 1.08,
        start_time: current_time,
        duration: 100.0,
        is_animating: true,
    };
}

fn create_bonus_label(tile: &TileData, bonus_score: u32, current_time: f64) -> BonusLabel {
    let lane_center_x = tile.x + tile.width / 2.0;
    let label_base_y = tile.y - 36.0 - 8.0;
    BonusLabel {
        x: lane_center_x,
        base_y: label_base_y,
        text: format!("+{}", bonus_score),
        animation: BonusLabelAnimation {
            scale: 1.0,
            opacity: 1.0,
            start_time: current_time,
            scale_duration: 250.0,
            fade_duration: 500.0,
            is_complete: false,
        },
    }
}

fn update_score_animation(score: &mut ScoreData, current_time: f64) {
    let anim = &mut score.animation;
    if !anim.is_animating {
        return;
    }
    let elapsed = current_time - anim.start_time;
    if elapsed < anim.duration {
        let progress = elapsed / anim.duration;
        anim.current_scale = 1.0 + 0.08 * progress as f32;
    } else if elapsed < anim.duration * 2.0 {
        let progress = (elapsed - anim.duration) / anim.duration;
        anim.current_scale = 1.08 - 0.08 * progress as f32;
    } else {
        anim.current_scale = 1.0;
        anim.is_animating = false;
    }
}

fn update_bonus_label_animations(score: &mut ScoreData, current_time: f64) {
    for label in &mut score.bonus_labels {
        let anim = &mut label.animation;
        if anim.is_complete {
            continue;
        }
        let elapsed = current_time - anim.start_time;
        if elapsed < anim.scale_duration {
            let progress = elapsed / anim.scale_duration;
            anim.scale = 1.0 + 0.08 * progress as f32;
        } else if elapsed < anim.scale_duration * 2.0 {
            let progress = (elapsed - anim.scale_duration) / anim.scale_duration;
            anim.scale = 1.08 - 0.08 * progress as f32;
        } else {
            anim.scale = 1.0;
        }
        if elapsed < anim.fade_duration {
            anim.opacity = 1.0 - (elapsed / anim.fade_duration) as f32;
        } else {
            anim.opacity = 0.0;
            anim.is_complete = true;
        }
    }
    score.bonus_labels.retain(|l| !l.animation.is_complete);
}
