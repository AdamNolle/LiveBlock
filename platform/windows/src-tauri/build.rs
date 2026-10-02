fn main() {
    // The context macro embeds the shared frontend at compile time. Track the
    // directory so UI-only builds cannot leave the executable with old assets.
    println!("cargo:rerun-if-changed=../../_shared-frontend/dist");
    tauri_build::build()
}
