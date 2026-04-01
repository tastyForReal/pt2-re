use std::collections::{HashMap, HashSet};

use super::bitmap_font_renderer::BitmapFontRenderer;
use super::gpu_context::GpuContext;
use super::score_renderer::ScoreRenderer;
use super::sprite_renderer::{RenderSpriteOptions, SpriteRenderer};
use super::tile_renderer::{TileRenderer, TileVertex, push_rect};
use crate::game::score_types::ScoreData;
use crate::game::types::*;

const FADE_DURATION: f64 = 300.0;
const DOT_DURATION: f64 = 300.0;
const PEAK_TIME: f64 = 50.0;
const CIRCLE_DURATION: f64 = 300.0;
const ANIM_FRAME_TIME: f64 = 30.0;

pub struct Renderer {
    pub tile_renderer: TileRenderer,
    pub font_renderer: BitmapFontRenderer,
    pub score_renderer: ScoreRenderer,
    pub sprite_renderer: SpriteRenderer,
    pub sprite_size_cache: HashMap<String, (f32, f32)>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            tile_renderer: TileRenderer::new(device, format),
            font_renderer: BitmapFontRenderer::new(),
            score_renderer: ScoreRenderer::new(),
            sprite_renderer: SpriteRenderer::new(),
            sprite_size_cache: HashMap::new(),
        }
    }

    pub fn initialize_font(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        assets_dir: &str,
    ) {
        let font_path = format!("{}/images/fonts/SofiaSansExtraCondensed.fnt", assets_dir);
        self.font_renderer
            .initialize(device, queue, format, &font_path);

        // Also initialize SpriteRenderer!
        self.sprite_renderer
            .initialize(device, queue, format, assets_dir);

        // Populate size cache
        if let Some(sheet) = &self.sprite_renderer.sheet_data {
            self.sprite_size_cache.clear();
            for (name, frame) in &sheet.frames {
                self.sprite_size_cache.insert(
                    name.clone(),
                    (frame.source_size.x as f32, frame.source_size.y as f32),
                );
            }
        }
    }

    fn get_sprite_size(&self, name: &str) -> (f32, f32) {
        *self.sprite_size_cache.get(name).unwrap_or(&(0.0, 0.0))
    }

    pub fn render_frame(
        &mut self,
        gpu: &GpuContext,
        game_data: &GameData,
        score_data: &ScoreData,
        now: f64,
        show_red_note_indicators: bool,
    ) -> Result<(), wgpu::SurfaceError> {
        let output = gpu.surface.get_current_texture()?;
        let view = output.texture.create_view(&Default::default());

        let actual_w = gpu.config.width as f32;
        let actual_h = gpu.config.height as f32;
        let scale_h = actual_h / SCREEN_HEIGHT;
        let scale_w = actual_w / SCREEN_WIDTH;

        self.tile_renderer
            .update_uniforms(&gpu.queue, actual_w, actual_h);
        self.sprite_renderer
            .update_uniforms(&gpu.queue, actual_w, actual_h);
        self.font_renderer
            .update_uniforms(&gpu.queue, actual_w, actual_h);

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Frame Encoder"),
            });

        let mut all_vertices: Vec<TileVertex> = Vec::new();

        // 1. Background
        push_rect(
            &mut all_vertices,
            0.0,
            0.0,
            actual_w,
            actual_h,
            [1.0, 1.0, 1.0, 1.0],
        );

        // Scan rows for indicators and start tile state
        let mut indicators_by_row: HashMap<usize, Vec<&NoteIndicatorData>> = HashMap::new();
        for ind in &game_data.note_indicators {
            if ind.is_consumed {
                continue;
            }
            indicators_by_row
                .entry(ind.row_index)
                .or_default()
                .push(ind);
        }

        let mut start_tile_pressed = false;
        let mut start_tile_data: Option<(f32, f32, f32, f32)> = None;

        for row in &game_data.rows {
            if row.row_type == RowType::StartingTileRow {
                for tile in &row.tiles {
                    start_tile_data = Some((tile.x, tile.y, tile.width, tile.height));
                    if tile.is_pressed {
                        start_tile_pressed = true;
                    }
                }
            }
        }

        // 2. Game over indicator (layer 1)
        if let Some(flash) = &game_data.game_over_data {
            let gy = (flash.tile.y + game_data.scroll_offset) * scale_h;
            let gh = flash.tile.height * scale_h;
            if gy + gh > 0.0 && gy < actual_h {
                let eff_opacity = if flash.tile.flash_state {
                    0.0
                } else {
                    flash.tile.opacity
                };
                let color = flash.tile.color.to_normalized(eff_opacity);
                push_rect(
                    &mut all_vertices,
                    flash.tile.x * scale_w,
                    gy,
                    flash.tile.width * scale_w,
                    gh,
                    color,
                );
            }
        }

        let layer1_vertex_count = all_vertices.len();

        // 3. Grid lines
        let grid_positions = [
            0.0,
            COLUMN_WIDTH * scale_w,
            COLUMN_WIDTH * 2.0 * scale_w,
            COLUMN_WIDTH * 3.0 * scale_w,
        ];
        for &x in &grid_positions {
            push_rect(
                &mut all_vertices,
                x,
                0.0,
                GRID_LINE_WIDTH,
                actual_h,
                [0.0, 0.0, 0.0, 1.0],
            );
        }

        // 4. Red note indicators
        if show_red_note_indicators {
            let mut seen_keys = HashSet::new();
            for ind in &game_data.note_indicators {
                let key = format!("{}_{}", ind.row_index, ind.time);
                if !seen_keys.insert(key) {
                    continue;
                }

                let sy = (ind.y + game_data.scroll_offset) * scale_h;
                let sh = ind.height * scale_h;
                if sy + sh > 0.0 && sy < actual_h {
                    push_rect(
                        &mut all_vertices,
                        ind.x * scale_w,
                        sy,
                        ind.width * scale_w,
                        sh,
                        [1.0, 0.0, 0.0, 1.0],
                    );
                }
            }
        }

        // 5. Batch Sprites
        self.sprite_renderer.begin_frame();
        if self.sprite_renderer.is_loaded() {
            let tile_base_w = self.get_sprite_size("tile_black.png").0;
            // Fallback to 134.0 if not found to avoid div by zero or panic
            let safe_base_w = if tile_base_w > 0.0 {
                tile_base_w
            } else {
                134.0
            };

            for row in &game_data.rows {
                for tile in &row.tiles {
                    let rect_y = (tile.y + game_data.scroll_offset) * scale_h;
                    let rect_h = tile.height * scale_h;
                    if rect_y + rect_h <= 0.0 || rect_y >= actual_h {
                        continue;
                    }

                    let is_long_tile =
                        row.height > BASE_ROW_HEIGHT && row.row_type != RowType::StartingTileRow;
                    let mut active_flash_state = tile.flash_state;
                    if let Some(flash) = &game_data.game_over_data {
                        // Sync flash state if this tile is the failing one
                        if (flash.tile.y - tile.y).abs() < 0.1
                            && flash.tile.lane_index == tile.lane_index
                        {
                            active_flash_state = flash.tile.flash_state;
                        }
                    }

                    let effective_opacity = if active_flash_state {
                        0.0
                    } else {
                        tile.opacity
                    };
                    let row_bottom = rect_y + rect_h;
                    let scale_sprite = tile.width / safe_base_w;

                    if is_long_tile {
                        self.render_long_tile(
                            tile,
                            row,
                            rect_y,
                            row_bottom,
                            scale_sprite,
                            effective_opacity,
                            game_data.scroll_offset,
                            now,
                            &indicators_by_row,
                            scale_w,
                            scale_h,
                        );
                    } else {
                        self.render_short_tile(
                            tile,
                            row,
                            rect_y,
                            effective_opacity,
                            now,
                            scale_w,
                            scale_h,
                        );
                    }
                }
            }
        }

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            // Draw layer 1
            self.tile_renderer.draw(
                &gpu.device,
                &mut render_pass,
                &all_vertices[..layer1_vertex_count],
            );

            // Draw Sprites
            if self.sprite_renderer.is_loaded() {
                self.sprite_renderer.draw(&gpu.device, &mut render_pass);
            }

            // Draw layer 2
            self.tile_renderer.draw(
                &gpu.device,
                &mut render_pass,
                &all_vertices[layer1_vertex_count..],
            );

            // START text
            self.font_renderer.begin_frame();
            if let Some((x, y, w, h)) = start_tile_data {
                if self.font_renderer.is_loaded() {
                    let text = "START";
                    let max_scale = 0.6;

                    let scale = self
                        .font_renderer
                        .calculate_scale_to_fit(text, w, h, max_scale);

                    let font_scale = scale * scale_w.min(scale_h);
                    let tx = (x + w * 0.5) * scale_w;
                    let ty = (y + h * 0.5) * scale_h;

                    let opacity = if start_tile_pressed { 0.0 } else { 1.0 };
                    self.font_renderer.render_text(
                        text,
                        tx,
                        ty,
                        font_scale,
                        [1.0, 1.0, 1.0, opacity],
                        0.0, // scroll offset already handled in ty calculation if needed, but START tile is fixed
                        &gpu.device,
                        &mut render_pass,
                        0.5,
                        0.5,
                    );
                }
            }

            if self.score_renderer.is_ready() {
                self.score_renderer.render(
                    &self.font_renderer,
                    score_data,
                    game_data.scroll_offset,
                    &gpu.device,
                    &mut render_pass,
                    actual_w,
                    actual_h,
                    scale_h,
                    scale_w,
                );
            }

            self.render_status_text(game_data, &gpu.device, &mut render_pass);
        }

        gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    fn render_long_tile(
        &mut self,
        rect: &TileData,
        row: &RowData,
        rect_y: f32,
        row_bottom: f32,
        scale: f32,
        effective_opacity: f32,
        scroll_offset: f32,
        now: f64,
        indicators_by_row: &HashMap<usize, Vec<&NoteIndicatorData>>,
        scale_w: f32,
        scale_h: f32,
    ) {
        let draw_w = rect.width * scale_w;
        let draw_x = rect.x * scale_w;
        let draw_h = rect.height * scale_h;

        if rect.is_pressed && !rect.is_released_early {
            self.sprite_renderer.batch_sprite(
                "long_finish.png",
                draw_x,
                rect_y,
                draw_w,
                draw_h,
                &RenderSpriteOptions {
                    opacity: effective_opacity,
                    ..Default::default()
                },
            );
        } else {
            self.sprite_renderer.batch_sprite(
                "long_tap2.png",
                draw_x,
                rect_y,
                draw_w,
                draw_h,
                &RenderSpriteOptions {
                    opacity: effective_opacity,
                    ..Default::default()
                },
            );
        }

        if (!rect.is_pressed && !rect.is_holding) || rect.is_released_early {
            let head_h_raw = self.get_sprite_size("long_head.png").1;
            let head_h = head_h_raw * scale * scale_w;
            let head_y = row_bottom - head_h;
            let trim_y = head_y + 2.0 * scale * scale_h;
            let scissor_y = rect_y.max(trim_y - 1.0);
            self.sprite_renderer.batch_sprite(
                "long_head.png",
                draw_x,
                head_y,
                draw_w,
                head_h,
                &RenderSpriteOptions {
                    opacity: effective_opacity,
                    scissor: Some([draw_x, scissor_y, draw_w, draw_h]),
                    ..Default::default()
                },
            );
        }

        let mut fade_opacity = 1.0;
        let mut should_render_progress = rect.progress > 0.0 && rect.progress < rect.height;
        if rect.is_pressed && !rect.is_released_early {
            if let Some(completed_at) = rect.completed_at {
                let elapsed = now - completed_at;
                if elapsed < FADE_DURATION {
                    fade_opacity = 1.0 - (elapsed / FADE_DURATION) as f32;
                    should_render_progress = true;
                }
            }
        }

        if should_render_progress {
            self.render_progress_effects(
                rect,
                rect_y,
                row_bottom,
                scale,
                effective_opacity,
                fade_opacity,
                now,
                scale_w,
                scale_h,
            );
        }

        if rect.is_holding {
            self.render_holding_indicators(
                rect,
                row,
                scale,
                effective_opacity,
                scroll_offset,
                indicators_by_row,
                scale_w,
                scale_h,
            );
        }
    }

    fn render_progress_effects(
        &mut self,
        rect: &TileData,
        rect_y: f32,
        row_bottom: f32,
        scale: f32,
        effective_opacity: f32,
        fade_opacity: f32,
        now: f64,
        scale_w: f32,
        scale_h: f32,
    ) {
        let prog_y = row_bottom - rect.progress * scale_h;
        let final_opacity = effective_opacity * fade_opacity;

        let long_light_h_raw = self.get_sprite_size("long_light.png").1;
        let light_head_h = long_light_h_raw * scale * scale_w;
        let light_head_y = prog_y - 8.0 * scale_h;
        let light_body_y = light_head_y + light_head_h;
        let light_body_h = (row_bottom - light_body_y).max(0.0);

        let draw_w = rect.width * scale_w;
        let draw_x = rect.x * scale_w;
        let draw_h = rect.height * scale_h;

        self.sprite_renderer.batch_sprite(
            "long_tilelight.png",
            draw_x,
            light_body_y,
            draw_w,
            light_body_h,
            &RenderSpriteOptions {
                opacity: final_opacity,
                scissor: Some([draw_x, rect_y, draw_w, draw_h]),
                ..Default::default()
            },
        );

        self.sprite_renderer.batch_sprite(
            "long_light.png",
            draw_x,
            light_head_y,
            draw_w,
            light_head_h,
            &RenderSpriteOptions {
                opacity: final_opacity,
                scissor: Some([draw_x, rect_y, draw_w, draw_h]),
                ..Default::default()
            },
        );

        if rect.last_note_played_at.is_some() {
            self.render_dot_animation(
                rect,
                light_head_y,
                scale,
                final_opacity,
                now,
                scale_w,
                scale_h,
            );
            self.render_circle_animations(
                rect,
                light_head_y,
                scale,
                final_opacity,
                now,
                scale_w,
                scale_h,
            );
        }
    }

    fn render_dot_animation(
        &mut self,
        rect: &TileData,
        light_head_y: f32,
        scale: f32,
        final_opacity: f32,
        now: f64,
        scale_w: f32,
        scale_h: f32,
    ) {
        let elapsed = now - rect.last_note_played_at.unwrap_or(0.0);
        if elapsed >= DOT_DURATION {
            return;
        }

        let anim_scale;
        let anim_opacity;

        if elapsed < PEAK_TIME {
            let t = elapsed / PEAK_TIME;
            anim_scale = 1.0 + 0.3 * t;
            anim_opacity = 1.0;
        } else {
            let t = (elapsed - PEAK_TIME) / (DOT_DURATION - PEAK_TIME);
            anim_scale = 1.3 * (1.0 - t);
            anim_opacity = 1.0 - t;
        }

        let dot_light_w_raw = self.get_sprite_size("dot_light.png").0;
        let base_dot_size = dot_light_w_raw * scale;
        self.sprite_renderer.batch_sprite(
            "dot_light.png",
            (rect.x + rect.width * 0.5) * scale_w,
            light_head_y,
            base_dot_size * anim_scale as f32 * scale_h, // Use scale_h for width
            base_dot_size * anim_scale as f32 * scale_h,
            &RenderSpriteOptions {
                opacity: (anim_opacity as f32) * final_opacity,
                anchor_x: 0.5,
                anchor_y: 0.5,
                ..Default::default()
            },
        );
    }

    fn render_circle_animations(
        &mut self,
        rect: &TileData,
        light_head_y: f32,
        scale: f32,
        final_opacity: f32,
        now: f64,
        scale_w: f32,
        scale_h: f32,
    ) {
        let circle_light_w_raw = self.get_sprite_size("circle_light.png").0;
        let base_circle_size = circle_light_w_raw * scale;
        for &start_time in &rect.active_circle_animations {
            let elapsed = now - start_time;
            if elapsed >= CIRCLE_DURATION {
                continue;
            }

            let t = elapsed / CIRCLE_DURATION;
            let circle_scale = 0.3 + t;
            let circle_opacity = 1.0 - t;

            self.sprite_renderer.batch_sprite(
                "circle_light.png",
                (rect.x + rect.width * 0.5) * scale_w,
                light_head_y,
                base_circle_size * circle_scale as f32 * scale_h, // Use scale_h for width
                base_circle_size * circle_scale as f32 * scale_h,
                &RenderSpriteOptions {
                    opacity: (circle_opacity as f32) * final_opacity,
                    anchor_x: 0.5,
                    anchor_y: 0.5,
                    ..Default::default()
                },
            );
        }
    }

    fn render_holding_indicators(
        &mut self,
        rect: &TileData,
        row: &RowData,
        scale: f32,
        effective_opacity: f32,
        scroll_offset: f32,
        indicators_by_row: &HashMap<usize, Vec<&NoteIndicatorData>>,
        scale_w: f32,
        scale_h: f32,
    ) {
        if let Some(row_inds) = indicators_by_row.get(&row.row_index) {
            let dot_size = 16.0 * scale;
            let center_x = (rect.x + rect.width * 0.5) * scale_w;
            let mut seen_times = HashSet::new();

            for ind in row_inds {
                let time_bits = ind.time.to_bits();
                if !seen_times.insert(time_bits) {
                    continue;
                }

                let dot_sy = (ind.y + scroll_offset) * scale_h;
                let dot_size_actual = dot_size * scale_h;
                let dot_draw_x = center_x - dot_size_actual * 0.5;
                if dot_sy + dot_size_actual > 0.0 && dot_sy < SCREEN_HEIGHT * scale_h {
                    self.sprite_renderer.batch_sprite(
                        "dot.png",
                        dot_draw_x,
                        dot_sy,
                        dot_size_actual,
                        dot_size_actual,
                        &RenderSpriteOptions {
                            opacity: effective_opacity,
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }

    fn render_short_tile(
        &mut self,
        rect: &TileData,
        row: &RowData,
        rect_y: f32,
        effective_opacity: f32,
        now: f64,
        scale_w: f32,
        scale_h: f32,
    ) {
        let mut frame_name = if row.row_type == RowType::StartingTileRow {
            "tile_start.png"
        } else {
            "tile_black.png"
        };

        if rect.is_pressed {
            if let Some(comp_at) = rect.completed_at {
                let frame_index = ((now - comp_at) / ANIM_FRAME_TIME) as i32;
                if frame_index == 1 {
                    frame_name = "1.png";
                } else if frame_index == 2 {
                    frame_name = "2.png";
                } else if frame_index == 3 {
                    frame_name = "3.png";
                } else if frame_index >= 4 {
                    frame_name = "4.png";
                }
            }
        }

        let draw_x = rect.x * scale_w;
        let draw_w = rect.width * scale_w;

        self.sprite_renderer.batch_sprite(
            frame_name,
            draw_x,
            rect_y,
            draw_w,
            rect.height * scale_h,
            &RenderSpriteOptions {
                opacity: effective_opacity,
                ..Default::default()
            },
        );
    }

    fn render_status_text(
        &self,
        _game_data: &GameData,
        _device: &wgpu::Device,
        _render_pass: &mut wgpu::RenderPass<'_>,
    ) {
    }
}
