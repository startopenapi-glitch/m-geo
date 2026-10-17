//! Explicit offline OS-vault test: only creates and removes its own random entry.
fn main() {
    assert_eq!(
        std::env::args().nth(1).as_deref(),
        Some("--offline-native-smoke")
    );
    let account = uuid::Uuid::new_v4().to_string();
    let value = format!("not-a-real-key-{}", uuid::Uuid::new_v4());
    let entry = keyring::Entry::new("M- GEO Native Smoke", &account).unwrap();
    let saved = entry.set_password(&value).is_ok();
    let matched = entry.get_password().is_ok_and(|stored| stored == value);
    let cleared = matches!(
        entry.delete_credential(),
        Ok(()) | Err(keyring::Error::NoEntry)
    );
    let absent = matches!(entry.get_password(), Err(keyring::Error::NoEntry));
    println!(
        "{}",
        serde_json::json!({"saved":saved,"round_trip":matched,"deleted":cleared,"absent_after_delete":absent,"network_requests":0})
    );
    if !(saved && matched && cleared && absent) {
        std::process::exit(1)
    }
}
