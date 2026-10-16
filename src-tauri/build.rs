fn main() {
    println!("cargo:rerun-if-changed=src/ssh_transport/meter.c");
    let mut build = cc::Build::new();
    build.file("src/ssh_transport/meter.c");
    if let Some(include) = std::env::var_os("DEP_SSH2_INCLUDE") {
        for path in std::env::split_paths(&include) {
            build.include(path);
        }
    }
    build.compile("sodeck_ssh_meter");
    tauri_build::build()
}
