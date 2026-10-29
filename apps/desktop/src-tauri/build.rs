fn main() {
    println!("cargo:rerun-if-env-changed=SODECK_BUILD_IDENTITY");
    let identity = if let Some(path) = std::env::var_os("SODECK_BUILD_IDENTITY") {
        println!("cargo:rerun-if-changed={}", std::path::Path::new(&path).display());
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).expect("Read build identity")).expect("Parse build identity");
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["appVersion"], std::env::var("CARGO_PKG_VERSION").unwrap());
        assert!(matches!(value["channel"].as_str(), Some("stable" | "preview")));
        assert!(value["buildId"].as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128));
        assert!(value["productName"].as_str().is_some_and(|s| !s.is_empty() && !s.contains(['\\', '/'])));
        assert!(value["identifier"].as_str().is_some_and(|s| s.starts_with("com.sodeck.v2")));
        assert!(value["catalogKeys"].as_object().is_some_and(|m| !m.is_empty()));
        assert!(value["updaterKey"].as_str().is_some_and(|s| !s.is_empty()));
        if value["qa"] == true {
            assert!(value["identifier"].as_str().unwrap().starts_with("com.sodeck.v2.qa."));
        } else {
            assert_eq!(value["identifier"], if value["channel"] == "stable" { "com.sodeck.v2" } else { "com.sodeck.v2.preview" });
        }
        value
    } else {
        serde_json::json!({"schemaVersion":1,"appVersion":std::env::var("CARGO_PKG_VERSION").unwrap(),"buildId":"development","channel":"development","productName":"SoDeck v2 Dev","identifier":"com.sodeck.v2.dev","catalogKeys":{},"updaterKey":null,"qa":false})
    };
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("build-identity.json");
    std::fs::write(output, serde_json::to_vec(&identity).unwrap()).unwrap();
    tauri_build::build();
}
