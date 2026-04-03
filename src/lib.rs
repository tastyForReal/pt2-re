#![allow(clippy::too_many_arguments)]
#![allow(clippy::new_without_default)]

pub mod game;
pub mod renderer;
pub mod video_recorder;

#[cfg(feature = "audio")]
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    improper_ctypes,
    clippy::all,
)]
mod miniaudio_bindings {
    include!(concat!(env!("OUT_DIR"), "/miniaudio_bindings.rs"));
}

#[cfg(feature = "soundfont")]
pub mod tsf_bindings;
