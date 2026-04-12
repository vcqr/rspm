fn main() {
    // 仅在 debug 模式下复制静态文件到 target 目录
    // Release 模式使用 rust-embed 嵌入到可执行文件内部
    #[cfg(debug_assertions)]
    {
        use std::fs;
        use std::path::PathBuf;

        // Get the output directory (target/debug or target/release)
        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");

        // Navigate from target/debug/build/rspm-daemon-*/out to target/debug
        let mut target_dir = PathBuf::from(&out_dir);
        // Go up from out -> build -> rspm-daemon-* -> debug/release
        target_dir.pop(); // out
        target_dir.pop(); // build
        target_dir.pop(); // rspm-daemon-*

        // Create static directory in target
        let static_dir = target_dir.join("static");
        fs::create_dir_all(&static_dir).expect("Failed to create static directory");

        // Copy static files from static/ to target/static
        let src_static_dir = std::path::Path::new("static");

        if src_static_dir.exists() {
            for entry in fs::read_dir(src_static_dir).expect("Failed to read static directory") {
                let entry = entry.expect("Failed to read entry");
                let path = entry.path();

                if path.is_file() {
                    let file_name = path.file_name().unwrap();
                    let dest_path = static_dir.join(file_name);

                    fs::copy(&path, &dest_path).expect("Failed to copy static file");
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
    }

    // 告诉 Cargo 如果 static 目录内容改变就重新编译
    println!("cargo:rerun-if-changed=static/");
}
