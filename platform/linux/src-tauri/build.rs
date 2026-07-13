fn main() {
    enforce_release_keyring();
    tauri_build::build()
}

fn enforce_release_keyring() {
    println!("cargo:rerun-if-changed=resources/trusted-model-keys.json");
    println!("cargo:rerun-if-env-changed=LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING");
    let json = std::fs::read_to_string("resources/trusted-model-keys.json")
        .expect("read trusted model keyring");
    let ring = liveblock_config::TrustedKeyringDocument::from_json(&json, false)
        .expect("trusted model keyring must satisfy the strict schema-1 runtime contract");
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }
    let explicit_development_override =
        std::env::var("LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING").as_deref() == Ok("1");
    if ring.is_empty() && !explicit_development_override {
        panic!("release packages require a nonempty schema-1 trusted model keyring; the empty-ring override is for explicit development/CI compilation only");
    }
}
