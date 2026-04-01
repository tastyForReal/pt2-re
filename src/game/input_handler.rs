use winit::event::{ElementState, KeyEvent, MouseButton};

use super::types::*;

/// Tracks the state of keyboard and mouse input for the game.
pub struct InputHandler {
    /// Which lanes are currently held down via keyboard (D, F, J, K).
    lane_held: [bool; 4],
    /// Mouse is currently pressed.
    mouse_pressed: bool,
    /// Last known mouse position in game coordinates.
    mouse_x: f32,
    mouse_y: f32,
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            lane_held: [false; 4],
            mouse_pressed: false,
            mouse_x: 0.0,
            mouse_y: 0.0,
        }
    }

    /// Process a keyboard event. Returns (lane, is_press) if the event maps to a game lane.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> Option<(u32, bool)> {
        let key = &event.logical_key;
        if let Some(lane) = key_to_lane(key) {
            let is_press = event.state == ElementState::Pressed;
            if is_press && !self.lane_held[lane as usize] {
                self.lane_held[lane as usize] = true;
                return Some((lane, true));
            } else if !is_press && self.lane_held[lane as usize] {
                self.lane_held[lane as usize] = false;
                return Some((lane, false));
            }
        }
        None
    }

    /// Process a mouse button event. Returns the lane under the cursor + press/release.
    pub fn handle_mouse_button(
        &mut self,
        state: ElementState,
        button: MouseButton,
    ) -> Option<(u32, bool)> {
        if button != MouseButton::Left {
            return None;
        }
        let is_press = state == ElementState::Pressed;
        self.mouse_pressed = is_press;
        let lane = mouse_x_to_lane(self.mouse_x);
        Some((lane, is_press))
    }

    pub fn mouse_y(&self) -> f32 {
        self.mouse_y
    }

    /// Update cursor position.
    pub fn update_cursor_position(&mut self, x: f32, y: f32) {
        self.mouse_x = x;
        self.mouse_y = y;
    }

    /// Release all held lanes (e.g., on focus loss).
    pub fn release_all(&mut self) -> Vec<u32> {
        let mut released = Vec::new();
        for i in 0..4 {
            if self.lane_held[i] {
                self.lane_held[i] = false;
                released.push(i as u32);
            }
        }
        if self.mouse_pressed {
            self.mouse_pressed = false;
        }
        released
    }

    pub fn is_any_lane_held(&self) -> bool {
        self.lane_held.iter().any(|&h| h) || self.mouse_pressed
    }
}

fn mouse_x_to_lane(x: f32) -> u32 {
    let lane = (x / COLUMN_WIDTH) as u32;
    lane.min(COLUMN_COUNT - 1)
}
