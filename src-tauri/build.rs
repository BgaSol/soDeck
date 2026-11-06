fn main() {
    println!(
        "cargo:rustc-env=SODECK_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    tauri_build::build();
    // Mock updater tests link TaskDialogIndirect through Tauri's dialog plugin. Unlike the
    // application binary, Rust's test executables do not inherit Tauri's Windows manifest.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var_os("CARGO_FEATURE_TEST_FIXTURES").is_some()
    {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
        // Tauri already supplies the application and fixture binaries' resource manifest.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
    }
}
