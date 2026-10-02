// Explicit loading returns a recoverable error when the runtime is missing.
// Letting ort implicitly initialize can panic before manual blocking starts.
use std::path::{Path, PathBuf};

pub fn initialize(resource_dir: &Path) -> anyhow::Result<()> {
    let filename = if cfg!(windows) { "onnxruntime.dll" }
        else if cfg!(target_os = "macos") { "libonnxruntime.dylib" }
        else { "libonnxruntime.so" };
    let library: PathBuf = std::env::var_os("ORT_DYLIB_PATH").map(PathBuf::from).unwrap_or_else(|| {
        [resource_dir.join("resources").join(filename), resource_dir.join(filename)]
            .into_iter().find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from(filename))
    });
    ort::init_from(&library).map_err(|e| anyhow::anyhow!("load {}: {e}", library.display()))?
        .with_name("LiveBlock").commit();
    Ok(())
}
