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
}
