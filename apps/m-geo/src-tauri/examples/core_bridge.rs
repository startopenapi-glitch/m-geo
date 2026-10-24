//! Test-only JSON-lines bridge. It is never included in the desktop binary.
use m_geo_lib::{ai, runtime};
use serde_json::{json, Value};
use std::{
    io::{BufRead, Write},
    sync::{Arc, Mutex},
};
#[tokio::main]
async fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("isolated test database path required");
    assert!(path.contains("mgeo-e2e-"), "refuse non-test database");
    let db = Arc::new(Mutex::new(
        runtime::open_database(std::path::Path::new(&path)).unwrap(),
    ));
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        let action = request["action"].as_str().unwrap_or("");
        let payload = request["payload"].clone();
        let result = match action {
            "ai.settings.get" | "ai.jobs" => ai::handle(db.clone(), action, payload).await,
            "app.info" => Ok(
                json!({"name":"M- GEO","version":env!("CARGO_PKG_VERSION"),"dataLocation":path,"platform":"browser+native-rust-test","releaseChannel":"test","updateUrl":"https://github.com/startopenapi-glitch/m-geo/releases"}),
            ),
            _ => runtime::core_dispatch(&mut db.lock().unwrap(), action, payload),
        };
        let response = match result {
            Ok(value) => json!({"ok":true,"value":value}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        println!("{response}");
        std::io::stdout().flush().unwrap();
    }
}
