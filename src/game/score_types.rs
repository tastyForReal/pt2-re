#[derive(Debug, Clone)]
pub struct ScoreAnimationState {
    pub current_scale: f32,
    pub target_scale: f32,
    pub start_time: f64,
    pub duration: f64,
    pub is_animating: bool,
}

impl Default for ScoreAnimationState {
    fn default() -> Self {
        Self {
            current_scale: 1.0,
            target_scale: 1.0,
            start_time: 0.0,
            duration: 100.0,
            is_animating: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BonusLabelAnimation {
    pub scale: f32,
    pub opacity: f32,
    pub start_time: f64,
    pub scale_duration: f64,
    pub fade_duration: f64,
    pub is_complete: bool,
}

#[derive(Debug, Clone)]
pub struct BonusLabel {
    pub x: f32,
    pub base_y: f32,
    pub text: String,
    pub animation: BonusLabelAnimation,
}

#[derive(Debug, Clone, Default)]
pub struct ScoreData {
    pub total_score: u32,
    pub animation: ScoreAnimationState,
    pub bonus_labels: Vec<BonusLabel>,
    pub override_display_text: Option<String>,
}
