fn main() {
    #[cfg(feature = "soundfont")]
    {
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

    // =====================================================================
    // miniaudio: compile C source and generate Rust bindings via bindgen.
    // Enabled whenever the "audio" feature is active.
    // =====================================================================
    #[cfg(feature = "audio")]
    {
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
        cc::Build::new()
            .file(&ma_c)
            .include(ma_dir)
            .define("MA_NO_RESOURCE_MANAGER", None)
            .define("MA_NO_NODE_GRAPH", None)
            // Disable MA_ASSERT() in release-like builds. On Windows, the
            // WASAPI worker thread can hit an assertion (ma_device_state_starting)
            // due to a timing sensitivity between the main thread's state
            // transition and the worker thread's wakeup. Disabling assertions
            // turns these into soft no-ops while preserving normal error
            // handling (ma_device_start still returns MA_INVALID_ARGS etc.).
            // The MA_ASSERT macro defaults to C assert(), which abort()s the
            // process — unrecoverable in a game context.
            .define("MA_ASSERT", "((void)0)")
            .opt_level(2)
            .compile("miniaudio");

        println!("cargo:rerun-if-changed=c_src/miniaudio/miniaudio.c");
        println!("cargo:rerun-if-changed=c_src/miniaudio/miniaudio.h");

        // Generate Rust bindings for miniaudio.h using bindgen.
        //
        // We use an allowlist to keep the generated bindings focused on the
        // APIs we actually use (device, decoder, result codes, and formats).
        // bindgen will transitively include any types referenced by the
        // allowlisted items.
        //
        // Include paths: clang (via libclang/bindgen) needs access to the
        // system C headers such as <stddef.h>, <stdint.h>, etc. On most
        // systems bindgen can discover these automatically. We only add
        // explicit paths on Linux where the defaults may not be sufficient.
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
            // Generate
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        // On Linux, clang may not find system headers automatically via
        // libclang. Add the standard GCC include paths. Skip on Windows/macOS
        // where bindgen and the system SDK handle this automatically.
        #[cfg(target_os = "linux")]
        {
            builder = builder
                .clang_arg("-I/usr/lib/gcc/x86_64-linux-gnu/14/include")
                .clang_arg("-I/usr/include")
                .clang_arg("-I/usr/include/x86_64-linux-gnu");
        }

        let bindings = builder.generate().expect("Unable to generate miniaudio bindings");

        let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        bindings
            .write_to_file(out_path.join("miniaudio_bindings.rs"))
            .expect("Couldn't write miniaudio bindings");
    }
}
