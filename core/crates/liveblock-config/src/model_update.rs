//! Crash-aware authenticated ONNX update coordinator.

use crate::model_manifest::{
    artifact_sha256, install_verified_artifact, ArtifactFormat, ModelManifest, ModelManifestError,
    TrustedKeyring,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const MODEL_UPDATE_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelUpdateState {
    pub schema_version: u32,
    pub highest_release_sequence: u64,
    pub accepted_manifest: ModelManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUpdateReceipt {
    pub model_id: String,
    pub model_version: String,
    pub release_sequence: u64,
    pub artifact_sha256: String,
    pub previous_artifact_preserved: bool,
}

#[derive(Debug, Error)]
pub enum ModelUpdateError {
    #[error(transparent)]
    Manifest(#[from] ModelManifestError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("update state JSON error: {0}")]
    StateJson(#[from] serde_json::Error),
    #[error("unsupported model update state schema {0}")]
    UnsupportedStateSchema(u32),
    #[error("model update state belongs to a different model")]
    ModelIdMismatch,
    #[error("release sequence {candidate} is not newer than accepted sequence {accepted}")]
    RollbackAttempt { candidate: u64, accepted: u64 },
    #[error("active model does not match authenticated update state")]
    ActiveStateMismatch,
    #[error("model activation validation failed: {0}")]
    Activation(String),
    #[error("update failed and rollback also failed: {0}")]
    RollbackFailed(String),
    #[error("model update state path has an unsupported type")]
    StatePathType,
    #[error("another model update transaction is active")]
    UpdateInProgress,
}

impl ModelUpdateState {
    pub fn load_optional(path: &Path) -> Result<Option<Self>, ModelUpdateError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ModelUpdateError::StatePathType);
        }
        let state: Self = serde_json::from_slice(&fs::read(path)?)?;
        if state.schema_version != MODEL_UPDATE_STATE_SCHEMA_VERSION {
            return Err(ModelUpdateError::UnsupportedStateSchema(
                state.schema_version,
            ));
        }
        state.accepted_manifest.validate_contract()?;
        if state.accepted_manifest.artifact_format != ArtifactFormat::Onnx {
            return Err(ModelUpdateError::ActiveStateMismatch);
        }
        if state.highest_release_sequence == 0
            || state.highest_release_sequence != state.accepted_manifest.release_sequence
        {
            return Err(ModelUpdateError::ActiveStateMismatch);
        }
        Ok(Some(state))
    }
}

/// Verify, install, production-load, then commit monotonic state. Any load/state
/// failure restores the previous active artifact.
pub fn apply_verified_file_update<F, A, T>(
    manifest: &ModelManifest,
    source: &Path,
    destination: &Path,
    state_path: &Path,
    trusted_keys: &TrustedKeyring,
    release_floor: Option<(&str, u64)>,
    validate: F,
    activate: A,
) -> Result<ModelUpdateReceipt, ModelUpdateError>
where
    F: FnOnce(&Path) -> Result<T, String>,
    A: FnOnce(T),
{
    manifest.verify(source, trusted_keys)?;
    if manifest.artifact_format != ArtifactFormat::Onnx {
        return Err(
            ModelManifestError::InvalidField("portable model update requires ONNX format").into(),
        );
    }
    let _transaction = UpdateLock::acquire(destination, state_path)?;
    if let Some((floor_model_id, floor_sequence)) = release_floor {
        if floor_model_id != manifest.model_id {
            return Err(ModelUpdateError::ModelIdMismatch);
        }
        if manifest.release_sequence <= floor_sequence {
            return Err(ModelUpdateError::RollbackAttempt {
                candidate: manifest.release_sequence,
                accepted: floor_sequence,
            });
        }
    }
    let previous_state = ModelUpdateState::load_optional(state_path)?;
    if let Some(state) = &previous_state {
        if state.accepted_manifest.model_id != manifest.model_id {
            return Err(ModelUpdateError::ModelIdMismatch);
        }
        if manifest.release_sequence <= state.highest_release_sequence {
            return Err(ModelUpdateError::RollbackAttempt {
                candidate: manifest.release_sequence,
                accepted: state.highest_release_sequence,
            });
        }
    }

    let backup = backup_path(destination)?;
    recover_active(destination, &backup, previous_state.as_ref())?;
    if let Some(state) = &previous_state {
        state.accepted_manifest.verify(destination, trusted_keys)?;
    }
    if path_present(&backup)? {
        require_regular(&backup).map_err(|_| ModelUpdateError::ActiveStateMismatch)?;
        fs::remove_file(&backup)?;
        sync_parent(&backup)?;
    }
    let backup = install_verified_artifact(manifest, source, destination, trusted_keys)?;
    let had_previous = path_present(&backup)?;
    let loaded = match validate(destination) {
        Ok(value) => value,
        Err(error) => {
            rollback(destination, &backup, had_previous)
                .map_err(|e| ModelUpdateError::RollbackFailed(e.to_string()))?;
            return Err(ModelUpdateError::Activation(error));
        }
    };
    let next = ModelUpdateState {
        schema_version: MODEL_UPDATE_STATE_SCHEMA_VERSION,
        highest_release_sequence: manifest.release_sequence,
        accepted_manifest: manifest.clone(),
    };
    if let Err(error) = persist_state(state_path, &next) {
        rollback(destination, &backup, had_previous)
            .map_err(|e| ModelUpdateError::RollbackFailed(e.to_string()))?;
        return Err(error);
    }
    // Activation remains inside the cross-process transaction lock so two
    // successful commands cannot commit disk state in one order and runtime
    // state in another.
    activate(loaded);
    Ok(ModelUpdateReceipt {
        model_id: manifest.model_id.clone(),
        model_version: manifest.model_version.clone(),
        release_sequence: manifest.release_sequence,
        artifact_sha256: manifest.artifact_sha256.clone(),
        previous_artifact_preserved: had_previous,
    })
}

struct UpdateLock {
    _file: fs::File,
}

impl UpdateLock {
    fn acquire(destination: &Path, state_path: &Path) -> Result<Self, ModelUpdateError> {
        let destination_parent = destination
            .parent()
            .ok_or(ModelManifestError::InvalidField("destination"))?;
        let state_parent = state_path
            .parent()
            .ok_or(ModelManifestError::InvalidField("state destination"))?;
        fs::create_dir_all(destination_parent)?;
        fs::create_dir_all(state_parent)?;
        let canonical_parent = fs::canonicalize(destination_parent)?;
        if canonical_parent != fs::canonicalize(state_parent)? {
            return Err(ModelManifestError::InvalidField(
                "artifact and update state must share a directory",
            )
            .into());
        }
        let name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(ModelManifestError::InvalidField("destination"))?;
        let path = canonical_parent.join(format!("{name}.transaction.lock"));
        if path_present(&path)? {
            require_regular(&path).map_err(|_| ModelUpdateError::UpdateInProgress)?;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                ModelUpdateError::UpdateInProgress
            } else {
                error.into()
            }
        })?;
        // The marker intentionally persists. The OS advisory lock, unlike a
        // create-new marker, is released by process death and is safe to reuse.
        Ok(Self { _file: file })
    }
}

fn recover_active(
    destination: &Path,
    backup: &Path,
    state: Option<&ModelUpdateState>,
) -> Result<(), ModelUpdateError> {
    let Some(state) = state else {
        return if path_present(destination)? || path_present(backup)? {
            Err(ModelUpdateError::ActiveStateMismatch)
        } else {
            Ok(())
        };
    };
    let accepted_hash = &state.accepted_manifest.artifact_sha256;
    if file_hash(destination).as_deref() == Some(accepted_hash) {
        return Ok(());
    }
    if file_hash(backup).as_deref() == Some(accepted_hash) {
        restore_backup(destination, backup)?;
        if file_hash(destination).as_deref() == Some(accepted_hash) {
            return Ok(());
        }
    }
    Err(ModelUpdateError::ActiveStateMismatch)
}

/// Recover an interrupted swap, then verify the persisted signed manifest and
/// active artifact before startup load. A first-install artifact with no state
/// is rolled back by deletion because this application-data path has no legacy
/// model contract. Malformed/future state remains untouched and fails closed.
pub fn recover_verified_active_manifest(
    state_path: &Path,
    artifact: &Path,
    trusted_keys: &TrustedKeyring,
) -> Result<Option<ModelManifest>, ModelUpdateError> {
    let _transaction = UpdateLock::acquire(artifact, state_path)?;
    let backup = backup_path(artifact)?;
    let Some(state) = ModelUpdateState::load_optional(state_path)? else {
        if path_present(&backup)? {
            return Err(ModelUpdateError::ActiveStateMismatch);
        }
        if path_present(artifact)? {
            require_regular(artifact).map_err(|_| ModelUpdateError::ActiveStateMismatch)?;
            fs::remove_file(artifact)?;
            sync_parent(artifact)?;
        }
        return Ok(None);
    };
    recover_active(artifact, &backup, Some(&state))?;
    state.accepted_manifest.verify(artifact, trusted_keys)?;
    Ok(Some(state.accepted_manifest))
}

fn file_hash(path: &Path) -> Option<String> {
    require_regular(path).ok()?;
    artifact_sha256(path).ok()
}

fn path_present(path: &Path) -> Result<bool, std::io::Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn require_regular(path: &Path) -> Result<(), std::io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(())
}

fn backup_path(destination: &Path) -> Result<PathBuf, ModelUpdateError> {
    let parent = destination
        .parent()
        .ok_or(ModelManifestError::InvalidField("destination"))?;
    let name = destination
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(ModelManifestError::InvalidField("destination"))?;
    Ok(parent.join(format!("{name}.pre-update")))
}

fn persist_state(path: &Path, state: &ModelUpdateState) -> Result<(), ModelUpdateError> {
    let parent = path
        .parent()
        .ok_or(ModelManifestError::InvalidField("state destination"))?;
    fs::create_dir_all(parent)?;
    if path_present(path)? {
        require_regular(path).map_err(|_| ModelUpdateError::StatePathType)?;
    }
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(ModelManifestError::InvalidField("state destination"))?;
    let temporary = parent.join(format!(".{name}.{}.installing", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), ModelUpdateError> {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        output.write_all(&serde_json::to_vec_pretty(state)?)?;
        output.write_all(b"\n")?;
        output.flush()?;
        output.sync_all()?;
        // The rename is the transaction commit point. A directory fsync failure
        // cannot safely be reported as pre-commit because rolling the model back
        // would then disagree with an already-visible new state file. The model
        // backup remains as recovery evidence if a later power loss drops this
        // directory entry.
        atomic_replace_state(&temporary, path)?;
        let _ = sync_parent(path);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn rollback(destination: &Path, backup: &Path, had_previous: bool) -> Result<(), ModelUpdateError> {
    if had_previous {
        restore_backup(destination, backup)?;
    } else if path_present(destination)? {
        require_regular(destination)?;
        fs::remove_file(destination)?;
        sync_parent(destination)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn restore_backup(destination: &Path, backup: &Path) -> Result<(), ModelUpdateError> {
    fs::rename(backup, destination)?;
    sync_parent(destination)?;
    Ok(())
}

#[cfg(windows)]
fn restore_backup(destination: &Path, backup: &Path) -> Result<(), ModelUpdateError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let result = unsafe {
        ReplaceFileW(
            wide(destination).as_ptr(),
            wide(backup).as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace_state(staging: &Path, destination: &Path) -> Result<(), ModelUpdateError> {
    fs::rename(staging, destination)?;
    Ok(())
}

#[cfg(windows)]
fn atomic_replace_state(staging: &Path, destination: &Path) -> Result<(), ModelUpdateError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let result = unsafe {
        MoveFileExW(
            wide(staging).as_ptr(),
            wide(destination).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn sync_parent(path: &Path) -> Result<(), ModelUpdateError> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
#[cfg(windows)]
fn sync_parent(_: &Path) -> Result<(), ModelUpdateError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::tempdir;

    fn manifest(path: &Path, key: &SigningKey, sequence: u64) -> ModelManifest {
        let mut value = ModelManifest {
            schema_version: crate::model_manifest::MODEL_MANIFEST_SCHEMA_VERSION,
            model_id: "liveblock-detector".into(),
            model_version: format!("1.0.{sequence}"),
            artifact_format: ArtifactFormat::Onnx,
            artifact_hash_algorithm: crate::model_manifest::ARTIFACT_HASH_ALGORITHM.into(),
            artifact_sha256: artifact_sha256(path).unwrap(),
            runtime_classes: crate::model_manifest::RUNTIME_CLASSES
                .iter()
                .map(|v| (*v).into())
                .collect(),
            input_width: 640,
            input_height: 640,
            nms_embedded: false,
            release_sequence: sequence,
            promotion_gate_schema: crate::model_manifest::PROMOTION_GATE_SCHEMA_VERSION,
            promotion_report_sha256: "cd".repeat(32),
            created_at: "2026-07-12T00:00:00Z".into(),
            key_id: "test".into(),
            signature: String::new(),
        };
        value.signature = BASE64.encode(key.sign(&value.signing_bytes().unwrap()).to_bytes());
        value
    }

    fn ring(key: &SigningKey) -> TrustedKeyring {
        TrustedKeyring::from([("test".into(), key.verifying_key())])
    }

    fn apply(
        path: &Path,
        source: &Path,
        key: &SigningKey,
        sequence: u64,
    ) -> Result<ModelUpdateReceipt, ModelUpdateError> {
        apply_verified_file_update(
            &manifest(source, key, sequence),
            source,
            &path.join("active.onnx"),
            &path.join("state.json"),
            &ring(key),
            None,
            |_| Ok(()),
            |_| {},
        )
    }

    #[test]
    fn published_state_schema_matches_runtime_contract() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/model-update-state.schema.json"
        ))
        .unwrap();
        assert_eq!(
            schema["properties"]["schemaVersion"]["const"],
            MODEL_UPDATE_STATE_SCHEMA_VERSION
        );
        assert_eq!(
            schema["properties"]["acceptedManifest"]["allOf"][0]["$ref"],
            "model-manifest.schema.json"
        );
        assert_eq!(
            schema["properties"]["acceptedManifest"]["allOf"][1]["properties"]["artifactFormat"]
                ["const"],
            "onnx"
        );
    }

    #[test]
    fn packaged_release_floor_rejects_older_first_update() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("v1.onnx");
        let key = SigningKey::from_bytes(&[31; 32]);
        fs::write(&source, b"v1").unwrap();
        assert!(matches!(
            apply_verified_file_update(
                &manifest(&source, &key, 1),
                &source,
                &dir.path().join("active.onnx"),
                &dir.path().join("state.json"),
                &ring(&key),
                Some(("liveblock-detector", 100)),
                |_| Ok(()),
                |_| {},
            ),
            Err(ModelUpdateError::RollbackAttempt { accepted: 100, .. })
        ));
        assert!(!dir.path().join("active.onnx").exists());
    }

    #[test]
    fn applies_then_rejects_same_sequence() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("v1.onnx");
        fs::write(&source, b"v1").unwrap();
        let key = SigningKey::from_bytes(&[21; 32]);
        assert_eq!(
            apply(dir.path(), &source, &key, 1)
                .unwrap()
                .release_sequence,
            1
        );
        assert!(matches!(
            apply(dir.path(), &source, &key, 1),
            Err(ModelUpdateError::RollbackAttempt { .. })
        ));
    }

    #[test]
    fn activation_failure_restores_model_and_state() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("v1.onnx");
        let second = dir.path().join("v2.onnx");
        fs::write(&first, b"v1").unwrap();
        fs::write(&second, b"broken").unwrap();
        let key = SigningKey::from_bytes(&[22; 32]);
        apply(dir.path(), &first, &key, 1).unwrap();
        let result = apply_verified_file_update(
            &manifest(&second, &key, 2),
            &second,
            &dir.path().join("active.onnx"),
            &dir.path().join("state.json"),
            &ring(&key),
            None,
            |_| Err::<(), _>("runtime rejected model".into()),
            |_| {},
        );
        assert!(matches!(result, Err(ModelUpdateError::Activation(_))));
        assert_eq!(fs::read(dir.path().join("active.onnx")).unwrap(), b"v1");
        assert_eq!(
            ModelUpdateState::load_optional(&dir.path().join("state.json"))
                .unwrap()
                .unwrap()
                .highest_release_sequence,
            1
        );
    }

    #[test]
    fn recovers_interrupted_swap_before_update() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("v1.onnx");
        let third = dir.path().join("v3.onnx");
        fs::write(&first, b"v1").unwrap();
        fs::write(&third, b"v3").unwrap();
        let key = SigningKey::from_bytes(&[23; 32]);
        apply(dir.path(), &first, &key, 1).unwrap();
        fs::rename(
            dir.path().join("active.onnx"),
            dir.path().join("active.onnx.pre-update"),
        )
        .unwrap();
        fs::write(dir.path().join("active.onnx"), b"uncommitted-v2").unwrap();
        apply(dir.path(), &third, &key, 3).unwrap();
        assert_eq!(fs::read(dir.path().join("active.onnx")).unwrap(), b"v3");
        assert_eq!(
            ModelUpdateState::load_optional(&dir.path().join("state.json"))
                .unwrap()
                .unwrap()
                .highest_release_sequence,
            3
        );
    }

    #[test]
    fn transaction_lock_is_held_through_runtime_activation() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("v1.onnx");
        let key = SigningKey::from_bytes(&[29; 32]);
        fs::write(&source, b"v1").unwrap();
        apply_verified_file_update(
            &manifest(&source, &key, 1),
            &source,
            &dir.path().join("active.onnx"),
            &dir.path().join("state.json"),
            &ring(&key),
            None,
            |_| Ok(()),
            |_| {
                let competing = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(dir.path().join("active.onnx.transaction.lock"))
                    .unwrap();
                assert!(fs2::FileExt::try_lock_exclusive(&competing).is_err());
            },
        )
        .unwrap();
    }

    #[test]
    fn startup_recovers_backup_and_reauthenticates_state() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("v1.onnx");
        let destination = dir.path().join("active.onnx");
        let state = dir.path().join("state.json");
        let key = SigningKey::from_bytes(&[26; 32]);
        fs::write(&first, b"v1").unwrap();
        apply(dir.path(), &first, &key, 1).unwrap();
        fs::rename(&destination, dir.path().join("active.onnx.pre-update")).unwrap();
        fs::write(&destination, b"uncommitted-v2").unwrap();
        let recovered = recover_verified_active_manifest(&state, &destination, &ring(&key))
            .unwrap()
            .unwrap();
        assert_eq!(recovered.release_sequence, 1);
        assert_eq!(fs::read(&destination).unwrap(), b"v1");
        let wrong_key = SigningKey::from_bytes(&[28; 32]);
        assert!(recover_verified_active_manifest(&state, &destination, &ring(&wrong_key)).is_err());

        fs::write(&destination, b"tampered").unwrap();
        assert!(recover_verified_active_manifest(&state, &destination, &ring(&key)).is_err());
    }

    #[test]
    fn startup_removes_uncommitted_first_install() {
        let dir = tempdir().unwrap();
        let destination = dir.path().join("active.onnx");
        fs::write(&destination, b"uncommitted-v1").unwrap();
        let key = SigningKey::from_bytes(&[27; 32]);
        assert!(recover_verified_active_manifest(
            &dir.path().join("state.json"),
            &destination,
            &ring(&key)
        )
        .unwrap()
        .is_none());
        assert!(!destination.exists());
    }

    #[test]
    fn future_state_fails_closed_without_rewrite() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("v1.onnx");
        let destination = dir.path().join("active.onnx");
        let state = dir.path().join("state.json");
        let key = SigningKey::from_bytes(&[30; 32]);
        fs::write(&source, b"v1").unwrap();
        apply(dir.path(), &source, &key, 1).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
        value["schemaVersion"] = serde_json::json!(2);
        let future = serde_json::to_vec(&value).unwrap();
        fs::write(&state, &future).unwrap();
        assert!(matches!(
            recover_verified_active_manifest(&state, &destination, &ring(&key)),
            Err(ModelUpdateError::UnsupportedStateSchema(2))
        ));
        assert_eq!(fs::read(&state).unwrap(), future);
        assert_eq!(fs::read(&destination).unwrap(), b"v1");
    }

    #[test]
    fn transaction_lock_serializes_sequence_checks() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("v1.onnx");
        let key = SigningKey::from_bytes(&[25; 32]);
        fs::write(&source, b"v1").unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.path().join("active.onnx.transaction.lock"))
            .unwrap();
        fs2::FileExt::lock_exclusive(&lock).unwrap();
        assert!(matches!(
            apply(dir.path(), &source, &key, 1),
            Err(ModelUpdateError::UpdateInProgress)
        ));
        drop(lock);
        assert_eq!(
            apply(dir.path(), &source, &key, 1)
                .unwrap()
                .release_sequence,
            1
        );
    }

    #[test]
    fn unknown_state_fields_and_untracked_active_fail_closed() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("v2.onnx");
        let key = SigningKey::from_bytes(&[24; 32]);
        fs::write(&source, b"v2").unwrap();
        fs::write(dir.path().join("active.onnx"), b"legacy").unwrap();
        assert!(matches!(
            apply(dir.path(), &source, &key, 2),
            Err(ModelUpdateError::ActiveStateMismatch)
        ));
        fs::remove_file(dir.path().join("active.onnx")).unwrap();
        fs::write(dir.path().join("state.json"), r#"{"schemaVersion":1,"modelId":"x","highestReleaseSequence":1,"artifactSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","extra":true}"#).unwrap();
        assert!(matches!(
            apply(dir.path(), &source, &key, 2),
            Err(ModelUpdateError::StateJson(_))
        ));
    }
}
