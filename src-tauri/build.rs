fn main() {
    println!(
        "cargo:rustc-env=SODECK_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    tauri_build::build();
}
