fn main() {
    enforce_release_keyring();
    tauri_build::build()
}

fn enforce_release_keyring() {
    println!("cargo:rerun-if-changed=resources/trusted-model-keys.json");
    println!("cargo:rerun-if-changed=resources/liveblock-detector.onnx");
    println!("cargo:rerun-if-changed=resources/liveblock-detector.manifest.json");
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
    if ring.is_empty() {
        if !explicit_development_override {
            panic!("release packages require a nonempty schema-1 trusted model keyring; the empty-ring override is for explicit development/CI compilation only");
        }
        return;
    }
    if explicit_development_override {
        panic!("the empty-ring override cannot accompany production Windows trust roots");
    }

    let artifact = std::path::Path::new("resources/liveblock-detector.onnx");
    let manifest_path = std::path::Path::new("resources/liveblock-detector.manifest.json");
    for path in [artifact, manifest_path] {
        let metadata = std::fs::symlink_metadata(path).unwrap_or_else(|error| {
            panic!(
                "production model resource {} is missing: {error}",
                path.display()
            )
        });
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            panic!(
                "production model resource {} must be a regular non-symlink file",
                path.display()
            );
        }
    }
    let manifest: liveblock_config::ModelManifest = serde_json::from_slice(
        &std::fs::read(manifest_path).expect("read production model manifest"),
    )
    .expect("parse strict production model manifest");
    if manifest.artifact_format != liveblock_config::ArtifactFormat::Onnx {
        panic!("production Windows model manifest must authenticate ONNX");
    }
    manifest.verify(artifact, &ring).expect(
        "production Windows model artifact and manifest must verify against the packaged keyring",
    );
}
