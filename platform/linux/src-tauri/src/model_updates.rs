//! Authenticated, rollback-resistant ONNX model updates.

use crate::{detection::Detector, paths, state::AppState};
use liveblock_config::{
    apply_verified_file_update, recover_verified_active_manifest, ArtifactFormat, ModelManifest,
    ModelUpdateReceipt, TrustedKeyring, TrustedKeyringDocument,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{AppHandle, Manager, State};

fn trusted_keys(app: &AppHandle) -> Result<TrustedKeyring, String> {
    let path = app
        .path()
        .resolve(
            "resources/trusted-model-keys.json",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|error| format!("resolve trusted model keyring: {error}"))?;
    let json = fs::read_to_string(&path)
        .map_err(|error| format!("read trusted model keyring {}: {error}", path.display()))?;
    TrustedKeyringDocument::from_json(&json, true).map_err(|error| error.to_string())
}

fn read_manifest(path: &Path) -> Result<ModelManifest, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("read manifest metadata: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("model manifest must be a regular file".into());
    }
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("parse model manifest: {error}"))
}

pub fn load_authenticated_active(app: &AppHandle) -> Result<Option<Detector>, String> {
    let artifact = paths::active_model_path();
    let state = paths::model_update_state_path();
    let backup = paths::models_dir().join("liveblock-detector.onnx.pre-update");
    if [&artifact, &state, &backup].iter().all(|path| {
        fs::symlink_metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }) {
        return Ok(None);
    }
    let keys = trusted_keys(app)?;
    let Some(active_manifest) = recover_verified_active_manifest(&state, &artifact, &keys)
        .map_err(|error| format!("verify active model update: {error}"))?
    else {
        return Ok(None);
    };
    if let Some((packaged, _)) = verified_packaged_manifest(app, &keys)? {
        if packaged.model_id != active_manifest.model_id {
            return Err("packaged and active detector model IDs differ".into());
        }
        if packaged.release_sequence > active_manifest.release_sequence {
            return Ok(None);
        }
        if packaged.release_sequence == active_manifest.release_sequence
            && packaged.artifact_sha256 != active_manifest.artifact_sha256
        {
            return Err("one release sequence authenticates two detector artifacts".into());
        }
    }
    Detector::load(&artifact)
        .map(Some)
        .map_err(|error| format!("load authenticated model update: {error}"))
}

fn verified_packaged_manifest(
    app: &AppHandle,
    keys: &TrustedKeyring,
) -> Result<Option<(ModelManifest, PathBuf)>, String> {
    let artifact = app
        .path()
        .resolve(
            "resources/liveblock-detector.onnx",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|error| format!("resolve packaged detector: {error}"))?;
    let manifest_path = app
        .path()
        .resolve(
            "resources/liveblock-detector.manifest.json",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|error| format!("resolve packaged detector manifest: {error}"))?;
    let artifact_exists = artifact.exists();
    let manifest_exists = manifest_path.exists();
    if !artifact_exists && !manifest_exists {
        return Ok(None);
    }
    if !artifact_exists || !manifest_exists {
        return Err("packaged detector and signed manifest must both be present".into());
    }
    let manifest = read_manifest(&manifest_path)?;
    if manifest.artifact_format != ArtifactFormat::Onnx {
        return Err("packaged portable detector manifest must use ONNX".into());
    }
    manifest
        .verify(&artifact, keys)
        .map_err(|error| error.to_string())?;
    Ok(Some((manifest, artifact)))
}

pub fn load_authenticated_packaged(app: &AppHandle) -> Result<Option<Detector>, String> {
    let keys = trusted_keys(app)?;
    let Some((_, artifact)) = verified_packaged_manifest(app, &keys)? else {
        return Ok(None);
    };
    Detector::load(&artifact)
        .map(Some)
        .map_err(|error| format!("load authenticated packaged detector: {error}"))
}

#[tauri::command]
pub fn install_model_update(
    manifest_path: PathBuf,
    artifact_path: PathBuf,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ModelUpdateReceipt, String> {
    let _update_guard = state.model_update.lock();
    let keys = trusted_keys(&app)?;
    let manifest = read_manifest(&manifest_path)?;
    let packaged = verified_packaged_manifest(&app, &keys)?;
    let release_floor = packaged
        .as_ref()
        .map(|(manifest, _)| (manifest.model_id.as_str(), manifest.release_sequence));
    let destination = paths::active_model_path();
    let update_state = paths::model_update_state_path();
    apply_verified_file_update(
        &manifest,
        &artifact_path,
        &destination,
        &update_state,
        &keys,
        release_floor,
        |path| Detector::load(path).map_err(|error| error.to_string()),
        |detector| *state.detector.lock() = Some(detector),
    )
    .map_err(|error| error.to_string())
}
