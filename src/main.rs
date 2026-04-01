use std::sync::Arc;

use clap::{Parser, ValueEnum};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

use pt2::game::game_controller::GameController;
use pt2::game::json_level_reader;
use pt2::game::types::*;
use pt2::renderer::gpu_context::GpuContext;
use pt2::renderer::renderer::Renderer;

/// Game mode selection for CLI.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliGameMode {
    OneRound,
    Endless,
    Survival,
}

/// Piano Tiles Re:U — a tile-tapping rhythm game.
#[derive(Parser, Debug)]
#[command(name = "pt2", about = "Rust FOSS clone of \"Piano Tiles 2\"")]
struct Cli {
    /// Path to a JSON level file to load on startup.
    #[arg(short, long)]
    file: Option<String>,

    /// Enable autoplay bot.
    #[arg(short, long, default_value_t = false)]
    autoplay: bool,

    /// Enable verbose logging.
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    /// Run in headless mode (no window, exit after loading).
    #[arg(long, default_value_t = false)]
    headless: bool,

    /// Pre-decode audio samples into memory on startup (uses more RAM but lower latency).
    #[arg(long, default_value_t = false)]
    preload_samples: bool,

    /// Game mode: one_round, endless, or survival.
    #[arg(long, value_enum)]
    gamemode: Option<CliGameMode>,

    /// Starting TPS values per section (comma-separated). For one_round/endless modes.
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    starting_tpses: Option<Vec<f32>>,

    /// Starting BPM values per section (comma-separated). Overrides --starting_tpses.
    /// For one_round/endless modes.
    #[arg(long, value_delimiter = ',', num_args = 1.., conflicts_with = "starting_tpses")]
    starting_bpms: Option<Vec<f64>>,

    /// Starting TPS value (single). For survival mode.
    #[arg(long)]
    starting_tps: Option<f32>,

    /// Starting BPM value (single). Overrides --starting_tps. For survival mode.
    #[arg(long, conflicts_with = "starting_tps")]
    starting_bpm: Option<f64>,

    /// Acceleration rate (TPS/sec) for survival mode. Defaults to 0.01.
    #[arg(long, default_value_t = 0.01)]
    acceleration_rate: f32,

    /// Path to a SoundFont (.sf2) file for synthesis playback.
    /// Default: assets/sounds/sf2/piano.sf2
    /// Precedence: CLI > built-in default
    #[cfg(feature = "soundfont")]
    #[arg(long)]
    soundfont: Option<String>,

    /// Enable soundfont synthesis (default: true).
    /// Use --no-soundfont to disable synthesis and use MP3 samples only.
    #[cfg(feature = "soundfont")]
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    soundfont_enabled: bool,

    /// Enable automatic fallback to MP3 samples when soundfont fails to load (default: true).
    /// When set to false, errors during soundfont loading will cause the game
    /// to run without audio rather than falling back to MP3.
    #[cfg(feature = "soundfont")]
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    soundfont_fallback: bool,
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
    renderer: Option<Renderer>,
    game_controller: GameController,
    cli: Cli,
    assets_dir: String,
    #[cfg(feature = "soundfont")]
    soundfont_path: std::path::PathBuf,
}

/// Resolve soundfont path with cross-platform support.
///
/// Resolution precedence:
/// 1. CLI-provided absolute path
/// 2. CLI-provided relative to executable directory
/// 3. Built-in default relative to executable directory
/// 4. Built-in default relative to current working directory
#[cfg(feature = "soundfont")]
fn resolve_soundfont_path(cli_path: Option<&str>, exe_dir: &std::path::Path) -> std::path::PathBuf {
    // Priority 1: CLI absolute path
    if let Some(path) = cli_path {
        let p = std::path::PathBuf::from(path);
        if p.is_absolute() && p.exists() {
            log::info!("Using absolute soundfont path from CLI: {}", path);
            return p;
        }

        // CLI relative path - check against executable directory
        let exe_relative = exe_dir.join(path);
        if exe_relative.exists() {
            log::info!(
                "Using soundfont path relative to executable: {:?}",
                exe_relative
            );
            return exe_relative;
        }

        // CLI relative path - check against current directory
        let cwd_relative = std::path::PathBuf::from(path);
        if cwd_relative.exists() {
            log::info!("Using soundfont path relative to cwd: {:?}", cwd_relative);
            return cwd_relative;
        }

        // CLI path provided but not found - return anyway for error reporting
        log::warn!("CLI soundfont path not found: {}", path);
        return p;
    }

    // Priority 2: Built-in default relative to executable
    let default_exe = exe_dir.join("assets/sounds/sf2/piano.sf2");
    if default_exe.exists() {
        log::info!("Using default soundfont path relative to executable");
        return default_exe;
    }

    // Priority 3: Built-in default relative to current working directory
    let default_cwd = std::path::PathBuf::from("assets/sounds/sf2/piano.sf2");
    if default_cwd.exists() {
        log::info!("Using default soundfont path relative to current directory");
        return default_cwd;
    }

    // Nothing found - return default path for error reporting
    log::warn!("Default soundfont path not found; using built-in default");
    default_exe
}

impl App {
    fn new(cli: Cli) -> Self {
        let game_controller = GameController::new(cli.autoplay);

        // Determine assets directory relative to executable
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let assets_dir = if exe_dir.join("assets").exists() {
            exe_dir.join("assets").to_string_lossy().to_string()
        } else {
            "assets".to_string()
        };

        // Resolve soundfont path for soundfont builds
        #[cfg(feature = "soundfont")]
        let soundfont_path = resolve_soundfont_path(cli.soundfont.as_deref(), &exe_dir);

        Self {
            window: None,
            gpu: None,
            renderer: None,
            game_controller,
            cli,
            assets_dir,
            #[cfg(feature = "soundfont")]
            soundfont_path,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_attributes = Window::default_attributes()
            .with_title("Piano Tiles Re:U")
            .with_inner_size(winit::dpi::LogicalSize::new(
                SCREEN_WIDTH as f64,
                SCREEN_HEIGHT as f64,
            ))
            .with_resizable(true)
            .with_visible(false);

        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .expect("Failed to create window"),
        );

        // Center the window
        if let Some(monitor) = window.current_monitor() {
            let screen_size = monitor.size();
            let window_size = window.outer_size();
            let x = (screen_size.width.saturating_sub(window_size.width)) / 2;
            let y = (screen_size.height.saturating_sub(window_size.height)) / 2;
            window.set_outer_position(winit::dpi::PhysicalPosition::new(x as i32, y as i32));
        }

        // Initialize GPU
        let gpu = match pollster::block_on(GpuContext::new(window.clone())) {
            Ok(g) => g,
            Err(e) => {
                log::error!("Failed to initialize GPU: {}", e);
                if self.cli.headless {
                    event_loop.exit();
                }
                panic!("Failed to initialize GPU: {}", e);
            }
        };

        let mut renderer = Renderer::new(&gpu.device, gpu.format);
        renderer.initialize_font(&gpu.device, &gpu.queue, gpu.format, &self.assets_dir);

        // Initialize audio samples
        self.game_controller
            .audio_manager
            .initialize_samples(&self.assets_dir, self.cli.preload_samples);

        // Initialize soundfont synthesis (soundfont feature only)
        #[cfg(feature = "soundfont")]
        {
            if self.cli.soundfont_enabled {
                let sf_path = self.soundfont_path.to_string_lossy();

                match self
                    .game_controller
                    .audio_manager
                    .load_soundfont(&sf_path, self.cli.soundfont_fallback)
                {
                    Ok(()) => {
                        log::info!(
                            "Soundfont loaded successfully: {:?} (fallback={})",
                            self.soundfont_path,
                            self.cli.soundfont_fallback
                        );
                    }
                    Err(e) => {
                        log::error!("Failed to load soundfont '{}': {}", sf_path, e);
                        if !self.cli.soundfont_fallback {
                            log::warn!("Fallback disabled; audio playback may not work");
                        } else {
                            log::info!("Falling back to MP3 sample playback");
                        }
                    }
                }
            } else {
                log::info!("Soundfont synthesis disabled via CLI flag");
            }

            // Log soundfont diagnostic info
            log::debug!(
                "{}",
                self.game_controller.audio_manager.get_soundfont_info()
            );
        }

        // Load level from CLI if provided
        if let Some(file_path) = self.cli.file.clone() {
            match self.load_level_with_gamemode(&file_path) {
                Ok(()) => log::info!("Loaded level from: {}", file_path),
                Err(e) => {
                    log::error!("Failed to load level: {}", e);
                    if self.cli.headless {
                        event_loop.exit();
                        return;
                    }
                }
            }
        }

        window.set_visible(!self.cli.headless);

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.renderer = Some(renderer);

        // Update window title if level was loaded
        self.update_window_title();

        if self.cli.headless {
            log::info!("Headless mode: Game completely loaded successfully. Exiting.");
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_keyboard_event(event, event_loop);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                self.game_controller.handle_mouse_input(state, button);
            }

            WindowEvent::CursorMoved { position, .. } => {
                if let Some(ref window) = self.window {
                    let size = window.inner_size();
                    let actual_w = size.width as f32;
                    let actual_h = size.height as f32;
                    let scale_h = actual_h / SCREEN_HEIGHT;
                    let scale_w = actual_w / SCREEN_WIDTH;

                    self.game_controller.handle_cursor_moved(
                        position.x as f32 / scale_w,
                        position.y as f32 / scale_h,
                    );
                }
            }

            WindowEvent::Focused(focused) => {
                if !focused {
                    self.game_controller.handle_focus_lost();
                }
            }

            WindowEvent::Resized(new_size) => {
                if let Some(ref mut gpu) = self.gpu {
                    gpu.resize(new_size.width, new_size.height);
                }
            }

            WindowEvent::RedrawRequested => {
                // Update game logic
                self.game_controller.update();

                // Render
                if let (Some(gpu), Some(renderer)) = (&mut self.gpu, &mut self.renderer) {
                    let mut score_data =
                        self.game_controller.score_manager.get_score_data().clone();
                    if self.game_controller.game_data.game_mode == GameMode::Survival {
                        score_data.override_display_text =
                            Some(format!("{:.3}", self.game_controller.game_data.current_tps));
                        score_data.animation.current_scale = 1.0;
                    }

                    let now_ms = self.game_controller.get_current_time_ms();
                    match renderer.render_frame(
                        gpu,
                        &self.game_controller.game_data,
                        &score_data,
                        now_ms,
                        false,
                    ) {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost) => {
                            gpu.resize(gpu.config.width, gpu.config.height);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            log::error!("Out of GPU memory!");
                            event_loop.exit();
                        }
                        Err(e) => {
                            log::warn!("Surface error: {:?}", e);
                        }
                    }
                }

                // Request next frame
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }
}

impl App {
    /// Load a level file with optional gamemode overrides from CLI args.
    /// Mirrors the TypeScript customize_dialog.ts + renderer_main.ts flow:
    /// - Parses the JSON level file to get LevelData
    /// - Applies custom TPS/BPM overrides to each music section
    /// - Constructs the appropriate GameMode and EndlessConfig
    /// - Calls load_level with the modified data
    fn load_level_with_gamemode(&mut self, path: &str) -> Result<(), String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file {}: {}", path, e))?;
        let json: serde_json::Value =
            serde_json::from_str(&contents).map_err(|e| format!("Invalid JSON: {}", e))?;
        let mut level_data = json_level_reader::load_level_from_json(&json)?;

        let filename = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();

        let cli_mode = self.cli.gamemode;
        let game_mode = match cli_mode {
            Some(CliGameMode::OneRound) | None => GameMode::OneRound,
            Some(CliGameMode::Endless) => GameMode::Endless,
            Some(CliGameMode::Survival) => GameMode::Survival,
        };

        let mut endless_config: Option<EndlessConfig> = None;

        match game_mode {
            GameMode::OneRound | GameMode::Endless => {
                // Apply per-section TPS overrides from --starting_tpses or --starting_bpms
                if let Some(ref bpm_values) = self.cli.starting_bpms {
                    // Convert BPM values to TPS using each section's base_beats
                    for (i, music) in level_data.musics.iter_mut().enumerate() {
                        if let Some(&bpm) = bpm_values.get(i) {
                            music.tps = (bpm / music.base_beats / 60.0) as f32;
                            log::info!(
                                "Section {} TPS overridden via BPM {}: {:.4}",
                                i,
                                bpm,
                                music.tps
                            );
                        }
                    }
                } else if let Some(ref tps_values) = self.cli.starting_tpses {
                    for (i, music) in level_data.musics.iter_mut().enumerate() {
                        if let Some(&tps) = tps_values.get(i) {
                            music.tps = tps;
                            log::info!("Section {} TPS overridden: {:.4}", i, tps);
                        }
                    }
                }
                // else: no overrides, respect level's own baseBpm/bpm values

                if game_mode == GameMode::Endless {
                    let fixed_tps_values: Vec<f32> =
                        level_data.musics.iter().map(|m| m.tps).collect();
                    endless_config = Some(EndlessConfig {
                        mode: GameMode::Endless,
                        fixed_tps_values: Some(fixed_tps_values),
                        starting_tps: None,
                        acceleration_rate: None,
                    });
                }
            }
            GameMode::Survival => {
                // Determine starting TPS from --starting_bpm, --starting_tps, or level default
                let first_base_beats = level_data
                    .musics
                    .first()
                    .map(|m| m.base_beats)
                    .unwrap_or(1.0);

                let starting_tps = if let Some(bpm) = self.cli.starting_bpm {
                    let tps = (bpm / first_base_beats / 60.0) as f32;
                    log::info!("Survival starting TPS from BPM {}: {:.4}", bpm, tps);
                    Some(tps)
                } else if let Some(tps) = self.cli.starting_tps {
                    log::info!("Survival starting TPS: {:.4}", tps);
                    Some(tps)
                } else {
                    // No explicit starting TPS — use None so create_game_data
                    // falls back to level's initial TPS from music metadata
                    None
                };

                let acceleration_rate = self.cli.acceleration_rate;
                log::info!("Survival acceleration rate: {:.4}", acceleration_rate);

                endless_config = Some(EndlessConfig {
                    mode: GameMode::Survival,
                    fixed_tps_values: None,
                    starting_tps,
                    acceleration_rate: Some(acceleration_rate),
                });
            }
        }

        if cli_mode.is_some() {
            log::info!("Game mode: {:?}", game_mode);
        }

        self.game_controller
            .load_level(level_data, game_mode, endless_config, filename);
        Ok(())
    }

    fn handle_keyboard_event(&mut self, event: KeyEvent, event_loop: &ActiveEventLoop) {
        if event.state != ElementState::Pressed {
            // Handle key release for game lanes
            self.game_controller.handle_key_input(&event);
            return;
        }

        // Handle special keys on press
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                log::info!("Escape pressed, exiting");
                event_loop.exit();
            }
            Key::Character(c) => match c.as_str() {
                "p" | "P" => {
                    self.game_controller.toggle_pause();
                }
                "r" | "R" => {
                    self.game_controller.reset_random();
                    // Reload level if file was specified
                    if let Some(file_path) = self.cli.file.clone() {
                        let _ = self.load_level_with_gamemode(&file_path);
                        self.update_window_title();
                    }
                    log::info!("Game reset");
                }
                #[cfg(feature = "soundfont")]
                "s" | "S" => {
                    // Toggle soundfont at runtime (debug feature)
                    let currently_enabled = self.game_controller.audio_manager.is_using_soundfont();
                    self.game_controller
                        .audio_manager
                        .set_soundfont_enabled(!currently_enabled);
                    log::info!(
                        "Soundfont toggled: {} -> {}",
                        currently_enabled,
                        !currently_enabled
                    );
                }
                _ => {
                    // Forward to game input handler (D, F, J, K)
                    self.game_controller.handle_key_input(&event);
                }
            },
            _ => {
                self.game_controller.handle_key_input(&event);
            }
        }
    }

    fn update_window_title(&self) {
        if let Some(ref window) = self.window {
            let filename = &self.game_controller.game_data.current_filename;
            if !filename.is_empty() {
                window.set_title(&format!("{} - Piano Tiles Re:U", filename));
            } else {
                window.set_title("Piano Tiles Re:U");
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();

    // Initialize logger
    let base_level = if cli.verbose { "debug" } else { "info" };

    // Build log filter string
    #[cfg(feature = "soundfont")]
    let log_config = format!(
        "{},symphonia_bundle_mp3=warn,wgpu_hal=warn,pt2=info",
        base_level
    );
    #[cfg(not(feature = "soundfont"))]
    let log_config = format!("{},symphonia_bundle_mp3=warn,wgpu_hal=warn", base_level);

    unsafe {
        std::env::set_var("RUST_LOG", log_config);
    }
    env_logger::init();

    log::info!("Piano Tiles Re:U starting...");
    if cli.headless {
        log::info!("Headless mode enabled");
    }
    if let Some(ref file) = cli.file {
        log::info!("Level file: {}", file);
    }
    if cli.autoplay {
        log::info!("Autoplay enabled");
    }

    // Log soundfont configuration (if feature enabled)
    #[cfg(feature = "soundfont")]
    {
        if cli.soundfont_enabled {
            log::info!(
                "Soundfont: enabled (path={:?}, fallback={})",
                cli.soundfont,
                cli.soundfont_fallback
            );
        } else {
            log::info!("Soundfont: disabled");
        }
    }
    #[cfg(not(feature = "soundfont"))]
    {
        log::info!("Soundfont: not compiled in (enable 'soundfont' feature)");
    }

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new(cli);
    event_loop.run_app(&mut app).expect("Event loop error");
}
