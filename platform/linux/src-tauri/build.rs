fn main() {
    let development_empty_ring = enforce_release_keyring();
    enforce_release_ort_runtime(development_empty_ring);
    tauri_build::build()
}

fn enforce_release_keyring() -> bool {
    println!("cargo:rerun-if-changed=resources/trusted-model-keys.json");
    println!("cargo:rerun-if-env-changed=LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING");
    let json = std::fs::read_to_string("resources/trusted-model-keys.json")
        .expect("read trusted model keyring");
    let ring = liveblock_config::TrustedKeyringDocument::from_json(&json, false)
        .expect("trusted model keyring must satisfy the strict schema-1 runtime contract");
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return false;
    }
    let explicit_development_override =
        std::env::var("LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING").as_deref() == Ok("1");
    if ring.is_empty() && !explicit_development_override {
        panic!("release packages require a nonempty schema-1 trusted model keyring; the empty-ring override is for explicit development/CI compilation only");
    }
    ring.is_empty() && explicit_development_override
}

fn enforce_release_ort_runtime(development_empty_ring: bool) {
    println!("cargo:rerun-if-env-changed=LIVEBLOCK_ALLOW_UNPACKAGED_ORT");
    println!("cargo:rerun-if-changed=resources/onnxruntime");
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }
    if std::env::var("LIVEBLOCK_ALLOW_UNPACKAGED_ORT").as_deref() == Ok("1") {
        if development_empty_ring {
            return;
        }
        panic!("the unpackaged ONNX Runtime override requires the empty development keyring and cannot accompany production trust roots");
    }
    let root = std::path::Path::new("resources/onnxruntime");
    require_elf_shared_object(root.join("libonnxruntime.so"));
    require_nonempty_notice(root.join("THIRD-PARTY-NOTICES.txt"));
    let providers = [
        ("CARGO_FEATURE_CUDA", "libonnxruntime_providers_cuda.so"),
        ("CARGO_FEATURE_ROCM", "libonnxruntime_providers_rocm.so"),
        (
            "CARGO_FEATURE_OPENVINO",
            "libonnxruntime_providers_openvino.so",
        ),
        (
            "CARGO_FEATURE_TENSORRT",
            "libonnxruntime_providers_tensorrt.so",
        ),
    ];
    let selected: Vec<_> = providers
        .iter()
        .filter(|(feature, _)| std::env::var_os(feature).is_some())
        .collect();
    if selected.len() > 1 {
        panic!("select at most one optional Linux ONNX Runtime provider");
    }
    if let Some((_, provider_library)) = selected.first() {
        require_elf_shared_object(root.join("libonnxruntime_providers_shared.so"));
        require_elf_shared_object(root.join(provider_library));
    }
}

fn open_regular(path: &std::path::Path) -> std::fs::File {
    let metadata = std::fs::symlink_metadata(path).unwrap_or_else(|_| {
        panic!(
            "release package runtime file is missing: {}",
            path.display()
        )
    });
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        panic!(
            "release package runtime path must be a regular non-symlink file: {}",
            path.display()
        );
    }
    std::fs::File::open(path)
        .unwrap_or_else(|_| panic!("read release package runtime file: {}", path.display()))
}

fn require_nonempty_notice(path: std::path::PathBuf) {
    use std::io::Read as _;
    let mut contents = String::new();
    open_regular(&path)
        .read_to_string(&mut contents)
        .unwrap_or_else(|_| panic!("runtime notice must be UTF-8 text: {}", path.display()));
    if contents.trim().is_empty() {
        panic!(
            "release package runtime notice must be nonempty: {}",
            path.display()
        );
    }
}

fn require_elf_shared_object(path: std::path::PathBuf) {
    use std::io::Read as _;
    let mut bytes = [0u8; 20];
    open_regular(&path)
        .read_exact(&mut bytes)
        .unwrap_or_else(|_| panic!("runtime ELF header is truncated: {}", path.display()));
    if &bytes[..4] != b"\x7fELF"
        || bytes[4] != 2
        || bytes[5] != 1
        || u16::from_le_bytes([bytes[16], bytes[17]]) != 3
    {
        panic!(
            "release package runtime library must be a 64-bit little-endian ELF shared object: {}",
            path.display()
        );
    }
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    let expected_machine = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => 62,
        Ok("aarch64") => 183,
        Ok(architecture) => panic!("unsupported Linux package architecture: {architecture}"),
        Err(_) => panic!("CARGO_CFG_TARGET_ARCH is unavailable"),
    };
    if machine != expected_machine {
        panic!(
            "release package runtime library architecture does not match the Rust target: {}",
            path.display()
        );
    }
}
