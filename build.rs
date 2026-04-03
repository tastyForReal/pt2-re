fn main() {
    #[cfg(feature = "soundfont")]
    compile_tinysoundfont();

    #[cfg(feature = "audio")]
    compile_miniaudio();
}

// =============================================================================
// TinySoundFont
// =============================================================================

/// Compile TinySoundFont C library.
///
/// TinySoundFont is a single-file C library with no external dependencies,
/// so it compiles identically on all platforms without special handling.
#[cfg(feature = "soundfont")]
fn compile_tinysoundfont() {
    let c_src_dir = std::path::Path::new("c_src");
    let tsf_c = c_src_dir.join("tsf.c");
    let tsf_h = c_src_dir.join("tsf.h");

    if !tsf_c.exists() || !tsf_h.exists() {
        panic!(
            "TinySoundFont source files not found in c_src/. \
             Please ensure c_src/tsf.c and c_src/tsf.h exist."
        );
    }

    cc::Build::new()
        .file(&tsf_c)
        .include(c_src_dir)
        .opt_level(2)
        .compile("tinysoundfont");

    println!("cargo:rerun-if-changed=c_src/tsf.c");
    println!("cargo:rerun-if-changed=c_src/tsf.h");
}

// =============================================================================
// miniaudio
// =============================================================================

/// Compile miniaudio C source and generate Rust bindings via bindgen.
///
/// This function handles platform-specific concerns:
/// - **System library linking**: Each platform's default audio backend has
///   specific library dependencies (e.g. `libasound` on Linux, `AudioToolbox`
///   on macOS, `ole32` on Windows). These are declared via `cargo:rustc-link-lib`.
/// - **Include path discovery**: bindgen uses libclang to parse C headers.
///   libclang may not know the target compiler's system include directories,
///   especially when cross-compiling. We discover them by running the target
///   C compiler (respecting `CC` env var and `TARGET`) and passing the paths
///   to bindgen via `-I` flags.
/// - **Cross-compilation**: The `cc` crate automatically respects the `CC`,
///   `CXX`, `CARGO_TARGET_`, and `TARGET` environment variables. Our include
///   path discovery uses the same compiler, so it works for cross-compilation
///   out of the box.
#[cfg(feature = "audio")]
fn compile_miniaudio() {
    let ma_dir = std::path::Path::new("c_src/miniaudio");
    let ma_c = ma_dir.join("miniaudio.c");
    let ma_h = ma_dir.join("miniaudio.h");

    if !ma_c.exists() || !ma_h.exists() {
        panic!(
            "miniaudio source files not found in c_src/miniaudio/. \
             Please ensure c_src/miniaudio/miniaudio.c and \
             c_src/miniaudio/miniaudio.h exist."
        );
    }

    // Compile miniaudio.c
    //
    // MA_NO_RESOURCE_MANAGER, MA_NO_NODE_GRAPH: Disable unused features to
    // reduce code size and avoid unnecessary dependencies.
    //
    // MA_ASSERT → ((void)0): Disable miniaudio assertions in all builds.
    // On Windows, the WASAPI worker thread can hit a timing-sensitive
    // assertion (ma_device_state_starting) due to a race between the main
    // thread's state transition and the worker thread's wakeup. Disabling
    // assertions turns these into no-ops while preserving normal error
    // handling (ma_device_start still returns MA_INVALID_ARGS etc.).
    cc::Build::new()
        .file(&ma_c)
        .include(ma_dir)
        .define("MA_NO_RESOURCE_MANAGER", None)
        .define("MA_NO_NODE_GRAPH", None)
        // MA_ASSERT is a function-like macro: MA_ASSERT(condition).
        // To disable it, we redefine it as a no-op that accepts and discards
        // the condition argument. Using ((void)(0)) directly would expand to
        // ((void)(0))(condition) which is invalid C ("called object is not a
        // function"). Instead we provide a function-like macro definition.
        .define("MA_ASSERT(x)", "(void)(x)")
        .opt_level(2)
        .compile("miniaudio");

    println!("cargo:rerun-if-changed=c_src/miniaudio/miniaudio.c");
    println!("cargo:rerun-if-changed=c_src/miniaudio/miniaudio.h");

    // Link platform-specific system libraries required by miniaudio's audio
    // backends. Without these, linking will fail with unresolved symbols.
    link_platform_audio_libs();

    // Generate Rust bindings for miniaudio.h using bindgen.
    //
    // We use an allowlist to keep the generated bindings focused on the
    // APIs we actually use (device, decoder, result codes, and formats).
    // bindgen will transitively include any types referenced by the
    // allowlisted items.
    let mut builder = bindgen::Builder::default()
        .header(ma_h.to_str().unwrap())
        // Allowlist device types and functions
        .allowlist_type("ma_device")
        .allowlist_type("ma_device_config")
        .allowlist_type("ma_device_type")
        .allowlist_type("ma_device_callback_proc")
        .allowlist_function("ma_device_init")
        .allowlist_function("ma_device_start")
        .allowlist_function("ma_device_stop")
        .allowlist_function("ma_device_uninit")
        .allowlist_function("ma_device_config_init")
        // Allowlist decoder types and functions
        .allowlist_type("ma_decoder")
        .allowlist_type("ma_decoder_config")
        .allowlist_type("ma_decoder_output_format")
        .allowlist_function("ma_decoder_init_file")
        .allowlist_function("ma_decoder_read_pcm_frames")
        .allowlist_function("ma_decoder_uninit")
        .allowlist_function("ma_decoder_get_length_in_pcm_frames")
        .allowlist_function("ma_decoder_get_output_format")
        .allowlist_function("ma_decoder_config_init")
        // Allowlist result, format, and basic types
        .allowlist_type("ma_result")
        .allowlist_type("ma_format")
        .allowlist_type("ma_uint32")
        .allowlist_type("ma_uint64")
        .allowlist_type("ma_bool32")
        .allowlist_type("ma_thread_priority")
        .allowlist_var("MA_SUCCESS")
        .allowlist_var("MA_FORMAT_F32")
        .allowlist_var("MA_FORMAT_S16")
        .allowlist_var("ma_device_type_playback")
        .allowlist_var("ma_device_type_capture")
        .allowlist_var("ma_device_type_duplex")
        // Blocklist items we don't need to keep the output manageable
        .blocklist_type("ma_engine_.*")
        .blocklist_type("ma_resource_manager_.*")
        .blocklist_type("ma_node_graph_.*")
        .blocklist_type("ma_mutex_.*")
        // Emit build information for reproducibility
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    // Discover system include paths from the target C compiler and pass
    // them to bindgen. This ensures libclang can find system headers
    // (stddef.h, stdint.h, etc.) regardless of the host/target platform
    // or cross-compilation setup.
    //
    // If discovery fails (e.g. on MSVC targets where the compiler doesn't
    // support GCC-style `-v` output), we simply skip it and let bindgen
    // fall back to its own built-in header discovery.
    if let Some(include_paths) = discover_compiler_include_paths() {
        for path in &include_paths {
            builder = builder.clang_arg(format!("-I{}", path.display()));
        }
    }

    let bindings = builder
        .generate()
        .expect("Unable to generate miniaudio bindings");

    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("miniaudio_bindings.rs"))
        .expect("Couldn't write miniaudio bindings");
}

/// Link platform-specific system libraries required by miniaudio's default
/// audio backends.
///
/// | Platform | Backend  | Required Libraries                                  |
/// |----------|----------|-----------------------------------------------------|
/// | Linux    | ALSA     | `libasound`, `libdl`, `libm`, `libpthread`          |
/// | macOS    | CoreAudio| `AudioToolbox.framework`, `CoreFoundation.framework`|
/// | Windows  | WASAPI   | `ole32.lib` (+ `winmm.lib` for MinGW targets)       |
///
/// These are detected from `CARGO_CFG_TARGET_OS` and `CARGO_CFG_TARGET_ENV`,
/// which cargo sets automatically based on the target triple, so this
/// function handles cross-compilation correctly.
#[cfg(feature = "audio")]
fn link_platform_audio_libs() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    match target_os.as_str() {
        "linux" => {
            // ALSA is the default backend on Linux. libasound provides the
            // ALSA API. libdl is needed for dlopen/dlsym (dynamic backend
            // loading). libm and libpthread are standard POSIX dependencies.
            println!("cargo:rustc-link-lib=asound");
            println!("cargo:rustc-link-lib=dl");
            println!("cargo:rustc-link-lib=m");
            println!("cargo:rustc-link-lib=pthread");
        }
        "macos" => {
            // CoreAudio is the default backend on macOS. Both frameworks are
            // needed: AudioToolbox for audio device access and CoreFoundation
            // for core types (CFString, CFRunLoop, etc.).
            println!("cargo:rustc-link-lib=framework=AudioToolbox");
            println!("cargo:rustc-link-lib=framework=CoreFoundation");
        }
        "windows" => {
            // WASAPI is the default backend on Windows. ole32 provides COM
            // initialization (CoInitializeEx), which WASAPI requires. MinGW
            // targets also need winmm for multimedia timer support.
            println!("cargo:rustc-link-lib=ole32");
            if target_env == "gnu" {
                println!("cargo:rustc-link-lib=winmm");
            }
        }
        // For other platforms (e.g. BSD, emscripten), miniaudio may still
        // compile but audio backends will be unavailable or may need
        // additional libraries configured manually by the user.
        _ => {}
    }
}

/// Discover the default system include paths of the target C compiler.
///
/// This runs `cc -E -xc - -v` (or the compiler specified by the `CC` env
/// var) and parses the output to extract the system include directories.
/// The output format (with `#include <...> search starts here:` / `End of
/// search list.` markers) is used by GCC, Clang, and Apple Clang.
///
/// The discovered paths are passed to bindgen via `-I` flags so that
/// libclang can find system headers (`stddef.h`, `stdint.h`, etc.) when
/// parsing `miniaudio.h`.
///
/// # Cross-compilation
///
/// Uses `cc::Build::new().get_compiler()` to obtain the target C compiler,
/// which respects:
/// - `CC` environment variable (cross-compiler path)
/// - `CARGO_TARGET_<TRIPLE>_CC` for per-target overrides
/// - `TARGET` environment variable for the target triple
/// - `cc::Build`'s automatic toolchain detection
///
/// # Returns
///
/// `Some(paths)` if discovery succeeded, `None` if the compiler could not
/// be run or its output could not be parsed. When `None` is returned,
/// bindgen falls back to its own built-in header discovery.
#[cfg(feature = "audio")]
fn discover_compiler_include_paths() -> Option<Vec<std::path::PathBuf>> {
    // Use the same C compiler that cc::Build uses for compiling miniaudio.c.
    // This ensures we get the correct target compiler for cross-compilation.
    let compiler = cc::Build::new().get_compiler();
    let compiler_path = compiler.path().to_str()?.to_string();

    // Run the compiler in preprocessor-only mode with verbose output.
    // The `-v` flag causes it to print its search paths between:
    //   "#include <...> search starts here:"
    //   "End of search list."
    //
    // The `-xc -` tells it to treat stdin as C source (we give it empty
    // stdin). The actual preprocessed output goes to stdout; the include
    // paths appear on stderr.
    //
    // NOTE: We intentionally do NOT pass compiler.args() here. The cc crate
    // may include additional flags (e.g., offload target specs, linker flags)
    // that can produce extra stderr output or change the compiler's search
    // behavior. We only need the base compiler path — the `-E -xc - -v`
    // flags are sufficient to query system include directories.
    let output = std::process::Command::new(&compiler_path)
        .arg("-E")
        .arg("-xc")
        .arg("-")
        .arg("-v")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .output()
        .ok()?;

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Locate the include path section in the compiler's verbose output.
    let start_marker = "#include <...> search starts here:";
    let end_marker = "End of search list.";

    let start_idx = stderr.find(start_marker)?;
    let remaining = &stderr[start_idx + start_marker.len()..];
    let end_idx = remaining.find(end_marker)?;

    let section = &remaining[..end_idx];

    let paths: Vec<std::path::PathBuf> = section
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .collect();

    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}
