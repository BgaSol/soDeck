//! Headless UI test bridge to the actual catalog service. Never bundled.
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
fn main() {
    let core =
        sodeck::AppCore::open(std::env::args().nth(1).unwrap().into(), "test".into()).unwrap();
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let args = &request["args"];
        let result = match request["command"].as_str().unwrap() {
            "server_catalog" => Ok(core.server_catalog()),
            "change_catalog" => core.change_catalog(
                args["revision"].as_u64().unwrap(),
                serde_json::from_value(args["change"].clone()).unwrap(),
            ),
            "recover_catalog" => core.recover_catalog(),
            _ => Err("Unsupported test command".into()),
        };
        let response = match result {
            Ok(value) => json!({"value":value}),
            Err(error) => json!({"error":error}),
        };
        println!("{response}");
        io::stdout().flush().unwrap();
    }
}
