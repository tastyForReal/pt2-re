#![allow(clippy::too_many_arguments)]
#![allow(clippy::new_without_default)]

pub mod game;
pub mod renderer;
pub mod video_recorder;

#[cfg(feature = "soundfont")]
pub mod tsf_bindings;
