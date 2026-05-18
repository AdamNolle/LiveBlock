use std::path::PathBuf;

fn main() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bridge_src = crate_root.join("src/lib.rs");
    println!("cargo:rerun-if-changed={}", bridge_src.display());

    let out_dir = crate_root.join("generated");
    std::fs::create_dir_all(&out_dir).expect("create generated dir");

    swift_bridge_build::parse_bridges([bridge_src])
        .write_all_concatenated(&out_dir, "LiveBlockBridge");
}
