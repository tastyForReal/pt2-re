use super::bitmap_font_renderer::BitmapFontRenderer;
use crate::game::score_types::*;

const SCORE_FONT_SIZE: f32 = 80.0;
const BASE_FONT_SIZE: f32 = 128.0;
const SCORE_Y_PERCENT: f32 = 0.125;
const SCORE_TEXT_COLOR: [f32; 4] = [248.0 / 255.0, 98.0 / 255.0, 90.0 / 255.0, 1.0];
const SHADOW_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const SHADOW_OFFSET_X: f32 = 1.0;
const SHADOW_OFFSET_Y: f32 = 1.0;
const BONUS_FONT_SIZE: f32 = 72.0;
const BONUS_TEXT_COLOR: [f32; 4] = [40.0 / 255.0, 162.0 / 255.0, 252.0 / 255.0, 1.0];

fn font_scale(target: f32) -> f32 {
    target / BASE_FONT_SIZE
}

pub struct ScoreRenderer;

impl ScoreRenderer {
    pub fn new() -> Self {
        Self
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub fn render<'a>(
        &self,
        font: &'a BitmapFontRenderer,
        score_data: &ScoreData,
        scroll_offset: f32,
        device: &wgpu::Device,
        render_pass: &mut wgpu::RenderPass<'a>,
        actual_w: f32,
        actual_h: f32,
        scale_h: f32,
        scale_w: f32,
    ) {
        if !font.is_loaded() {
            return;
        }

        let default_text = format!("{}", score_data.total_score);
        let score_text = score_data
            .override_display_text
            .as_deref()
            .unwrap_or(&default_text);

        let scale = font_scale(SCORE_FONT_SIZE) * score_data.animation.current_scale * scale_w;
        let x = actual_w / 2.0;
        let y = actual_h * SCORE_Y_PERCENT;

        // Shadow
        font.render_text(
            score_text,
            x + SHADOW_OFFSET_X * scale_w,
            y + SHADOW_OFFSET_Y * scale_h,
            scale,
            SHADOW_COLOR,
            0.0,
            device,
            render_pass,
            0.5,
            0.5,
        );
        // Main
        font.render_text(
            score_text,
            x,
            y,
            scale,
            SCORE_TEXT_COLOR,
            0.0,
            device,
            render_pass,
            0.5,
            0.5,
        );

        // Bonus labels
        for label in &score_data.bonus_labels {
            let anim = &label.animation;
            let label_scale = font_scale(BONUS_FONT_SIZE) * anim.scale * scale_w;
            let color = [
                BONUS_TEXT_COLOR[0],
                BONUS_TEXT_COLOR[1],
                BONUS_TEXT_COLOR[2],
                anim.opacity,
            ];
            font.render_text(
                &label.text,
                label.x * scale_w,
                (label.base_y + scroll_offset) * scale_h,
                label_scale,
                color,
                0.0,
                device,
                render_pass,
                0.5,
                0.5,
            );
        }
    }
}
