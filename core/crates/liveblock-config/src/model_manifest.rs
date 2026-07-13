//! Authenticated model artifact contract shared by every desktop runtime.
//!
//! A manifest is accepted only when its schema/taxonomy is current, its
//! Ed25519 signature matches a configured trusted public key, and the exact
//! file or directory-tree hash matches the artifact. Installation stages and
//! re-hashes before an atomic rename; no private signing key belongs here.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const MODEL_MANIFEST_SCHEMA_VERSION: u32 = 2;
pub const MODEL_KEYRING_SCHEMA_VERSION: u32 = 1;
pub const PROMOTION_GATE_SCHEMA_VERSION: u32 = 5;
pub const ARTIFACT_HASH_ALGORITHM: &str = "sha256-file-or-tree-v1";
pub const RUNTIME_CLASSES: [&str; 3] = ["Logo", "Ad banner", "Sponsored"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    Coreml,
    Onnx,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelManifest {
    pub schema_version: u32,
    pub model_id: String,
    pub model_version: String,
    pub artifact_format: ArtifactFormat,
    pub artifact_hash_algorithm: String,
    pub artifact_sha256: String,
    pub runtime_classes: Vec<String>,
    pub input_width: u32,
    pub input_height: u32,
    pub nms_embedded: bool,
    pub release_sequence: u64,
    pub promotion_gate_schema: u32,
    pub promotion_report_sha256: String,
    pub created_at: String,
    pub key_id: String,
    pub signature: String,
}

pub type TrustedKeyring = BTreeMap<String, VerifyingKey>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedKeyringDocument {
    pub schema_version: u32,
    pub keys: Vec<TrustedPublicKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedPublicKey {
    pub key_id: String,
    pub public_key_base64: String,
}

#[derive(Debug, Error)]
pub enum ModelManifestError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported model manifest schema {0}")]
    UnsupportedSchema(u32),
    #[error("runtime class contract mismatch")]
    RuntimeClasses,
    #[error("invalid model manifest field: {0}")]
    InvalidField(&'static str),
    #[error("untrusted signing key id: {0}")]
    UntrustedKey(String),
    #[error("invalid base64 signature or public key")]
    SignatureEncoding,
    #[error("trusted model keyring is empty")]
    EmptyKeyring,
    #[error("duplicate trusted signing key id: {0}")]
    DuplicateKey(String),
    #[error("model manifest signature verification failed")]
    Signature,
    #[error("artifact fingerprint mismatch")]
    ArtifactFingerprint,
    #[error("install staging or backup path already exists: {0}")]
    InstallCollision(PathBuf),
}

impl ModelManifest {
    pub fn from_json(input: &str) -> Result<Self, ModelManifestError> {
        let manifest: Self = serde_json::from_str(input)?;
        manifest.validate_contract()?;
        Ok(manifest)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ModelManifestError> {
        Self::from_json(&fs::read_to_string(path)?)
    }

    pub fn validate_contract(&self) -> Result<(), ModelManifestError> {
        if self.schema_version != MODEL_MANIFEST_SCHEMA_VERSION {
            return Err(ModelManifestError::UnsupportedSchema(self.schema_version));
        }
        if self.model_id.trim().is_empty() {
            return Err(ModelManifestError::InvalidField("modelId"));
        }
        if self.model_version.trim().is_empty() {
            return Err(ModelManifestError::InvalidField("modelVersion"));
        }
        if self.input_width == 0 || self.input_height == 0 {
            return Err(ModelManifestError::InvalidField("input size"));
        }
        if self.artifact_hash_algorithm != ARTIFACT_HASH_ALGORITHM {
            return Err(ModelManifestError::InvalidField("artifactHashAlgorithm"));
        }
        if !is_lower_hex_sha256(&self.artifact_sha256) {
            return Err(ModelManifestError::InvalidField("artifactSha256"));
        }
        let expected: Vec<String> = RUNTIME_CLASSES.iter().map(|s| (*s).to_string()).collect();
        if self.runtime_classes != expected {
            return Err(ModelManifestError::RuntimeClasses);
        }
        if self.release_sequence == 0 {
            return Err(ModelManifestError::InvalidField("releaseSequence"));
        }
        if self.promotion_gate_schema != PROMOTION_GATE_SCHEMA_VERSION {
            return Err(ModelManifestError::InvalidField("promotionGateSchema"));
        }
        if !is_lower_hex_sha256(&self.promotion_report_sha256) {
            return Err(ModelManifestError::InvalidField("promotionReportSha256"));
        }
        if self.created_at.trim().is_empty() {
            return Err(ModelManifestError::InvalidField("createdAt"));
        }
        if self.key_id.trim().is_empty() {
            return Err(ModelManifestError::InvalidField("keyId"));
        }
        Ok(())
    }

    /// Stable compact JSON for the detached signature. `signature` itself is
    /// removed, and serde_json's map ordering provides lexicographic keys.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, ModelManifestError> {
        let mut value = serde_json::to_value(self)?;
        value
            .as_object_mut()
            .ok_or(ModelManifestError::InvalidField("manifest"))?
            .remove("signature");
        Ok(serde_json::to_vec(&value)?)
    }

    pub fn verify(
        &self,
        artifact: impl AsRef<Path>,
        trusted_keys: &TrustedKeyring,
    ) -> Result<(), ModelManifestError> {
        self.validate_contract()?;
        let key = trusted_keys
            .get(&self.key_id)
            .ok_or_else(|| ModelManifestError::UntrustedKey(self.key_id.clone()))?;
        let signature_bytes = BASE64
            .decode(&self.signature)
            .map_err(|_| ModelManifestError::SignatureEncoding)?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|_| ModelManifestError::SignatureEncoding)?;
        key.verify(&self.signing_bytes()?, &signature)
            .map_err(|_| ModelManifestError::Signature)?;
        if artifact_sha256(artifact.as_ref())? != self.artifact_sha256 {
            return Err(ModelManifestError::ArtifactFingerprint);
        }
        Ok(())
    }
}

impl TrustedKeyringDocument {
    pub fn from_json(
        input: &str,
        require_nonempty: bool,
    ) -> Result<TrustedKeyring, ModelManifestError> {
        let document: Self = serde_json::from_str(input)?;
        if document.schema_version != MODEL_KEYRING_SCHEMA_VERSION {
            return Err(ModelManifestError::UnsupportedSchema(
                document.schema_version,
            ));
        }
        if require_nonempty && document.keys.is_empty() {
            return Err(ModelManifestError::EmptyKeyring);
        }
        let mut keyring = TrustedKeyring::new();
        for entry in document.keys {
            if entry.key_id.trim().is_empty() {
                return Err(ModelManifestError::InvalidField("keyId"));
            }
            if keyring.contains_key(&entry.key_id) {
                return Err(ModelManifestError::DuplicateKey(entry.key_id));
            }
            let key = verifying_key_from_base64(&entry.public_key_base64)?;
            keyring.insert(entry.key_id, key);
        }
        Ok(keyring)
    }

    pub fn load(
        path: impl AsRef<Path>,
        require_nonempty: bool,
    ) -> Result<TrustedKeyring, ModelManifestError> {
        Self::from_json(&fs::read_to_string(path)?, require_nonempty)
    }
}

pub fn verifying_key_from_base64(encoded: &str) -> Result<VerifyingKey, ModelManifestError> {
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| ModelManifestError::SignatureEncoding)?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| ModelManifestError::SignatureEncoding)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| ModelManifestError::SignatureEncoding)
}

/// Hash a model file or directory tree. Directory paths use `/` separators so
/// manifests are stable across Windows, macOS, and Linux.
pub fn artifact_sha256(path: &Path) -> Result<String, ModelManifestError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(ModelManifestError::InvalidField("artifact symlink"));
    }
    if metadata.is_file() {
        return file_sha256(path);
    }
    if !metadata.is_dir() {
        return Err(ModelManifestError::InvalidField("artifact type"));
    }

    // Domain-separated, length-prefixed paths followed by fixed-size content
    // digests make tree boundaries unambiguous. Raw path+content concatenation
    // can collide across different file layouts.
    let mut digest = Sha256::new();
    digest.update(b"liveblock-tree-sha256-v1\0");
    let mut files = Vec::new();
    collect_files(path, path, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (relative, file) in files {
        let path_bytes = relative.as_bytes();
        digest.update((path_bytes.len() as u64).to_be_bytes());
        digest.update(path_bytes);
        let content = file_sha256_bytes(&file)?;
        digest.update(content);
    }
    Ok(to_lower_hex(&digest.finalize()))
}

/// Verify, uniquely stage, re-hash, and atomically replace a single-file model
/// artifact. ONNX is supported on every desktop. Directory packages are
/// authenticated by [`ModelManifest::verify`] but require a platform updater
/// with atomic directory-swap semantics and are rejected here.
///
/// The previous file remains at `<destination>.pre-update`. A create-new lock
/// serializes cooperating installers; stale locks fail closed for inspection.
pub fn install_verified_artifact(
    manifest: &ModelManifest,
    source: &Path,
    destination: &Path,
    trusted_keys: &TrustedKeyring,
) -> Result<PathBuf, ModelManifestError> {
    manifest.verify(source, trusted_keys)?;
    if manifest.artifact_format != ArtifactFormat::Onnx {
        return Err(ModelManifestError::InvalidField(
            "single-file installation requires ONNX format",
        ));
    }
    let source_metadata = fs::symlink_metadata(source)?;
    if !source_metadata.is_file() || source_metadata.file_type().is_symlink() {
        return Err(ModelManifestError::InvalidField(
            "atomic installation requires a regular file artifact",
        ));
    }
    let parent = destination
        .parent()
        .ok_or(ModelManifestError::InvalidField("destination"))?;
    fs::create_dir_all(parent)?;
    let canonical_parent = fs::canonicalize(parent)?;
    let name = destination
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(ModelManifestError::InvalidField("destination"))?;
    let canonical_destination = canonical_parent.join(name);
    if fs::canonicalize(source)? == canonical_destination {
        return Err(ModelManifestError::InvalidField(
            "source equals destination",
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ModelManifestError::InvalidField("destination type"));
        }
    }

    let lock_path = canonical_parent.join(format!("{name}.update.lock"));
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ModelManifestError::InstallCollision(lock_path.clone())
            } else {
                ModelManifestError::Io(error)
            }
        })?;
    let _lock = InstallLock {
        path: lock_path,
        _file: lock,
    };

    let nonce = uuid::Uuid::new_v4();
    let staging = canonical_parent.join(format!(".{name}.{nonce}.installing"));
    let backup = canonical_parent.join(format!("{name}.pre-update"));
    if backup.exists() {
        return Err(ModelManifestError::InstallCollision(backup));
    }

    let result = (|| {
        copy_file_create_new(source, &staging)?;
        if artifact_sha256(&staging)? != manifest.artifact_sha256 {
            return Err(ModelManifestError::ArtifactFingerprint);
        }
        atomic_replace_file(&staging, &canonical_destination, &backup)?;
        Ok(backup.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn to_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn file_sha256(path: &Path) -> Result<String, ModelManifestError> {
    Ok(to_lower_hex(&file_sha256_bytes(path)?))
}

fn file_sha256_bytes(path: &Path) -> Result<[u8; 32], ModelManifestError> {
    let mut digest = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

fn collect_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<(String, PathBuf)>,
) -> Result<(), ModelManifestError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(ModelManifestError::InvalidField("artifact symlink"));
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_files(root, &path, output)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ModelManifestError::InvalidField("artifact path"))?
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            output.push((relative, path));
        }
    }
    Ok(())
}

struct InstallLock {
    path: PathBuf,
    _file: fs::File,
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn copy_file_create_new(source: &Path, destination: &Path) -> Result<(), ModelManifestError> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.flush()?;
    output.sync_all()?;
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace_file(
    staging: &Path,
    destination: &Path,
    backup: &Path,
) -> Result<(), ModelManifestError> {
    if destination.exists() {
        copy_file_create_new(destination, backup)?;
        sync_parent(destination)?;
    }
    // POSIX rename replaces an existing regular file atomically. The backup
    // was fully synced while the old destination remained active.
    fs::rename(staging, destination)?;
    sync_parent(destination)?;
    Ok(())
}

#[cfg(not(windows))]
fn sync_parent(path: &Path) -> Result<(), ModelManifestError> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_replace_file(
    staging: &Path,
    destination: &Path,
    backup: &Path,
) -> Result<(), ModelManifestError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let staging_w = wide(staging);
    let destination_w = wide(destination);
    let result = if destination.exists() {
        let backup_w = wide(backup);
        unsafe {
            ReplaceFileW(
                destination_w.as_ptr(),
                staging_w.as_ptr(),
                backup_w.as_ptr(),
                REPLACEFILE_WRITE_THROUGH,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    } else {
        unsafe {
            MoveFileExW(
                staging_w.as_ptr(),
                destination_w.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if result == 0 {
        return Err(ModelManifestError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::tempdir;

    fn signed_manifest(artifact: &Path, key: &SigningKey) -> ModelManifest {
        let mut manifest = ModelManifest {
            schema_version: MODEL_MANIFEST_SCHEMA_VERSION,
            model_id: "liveblock-detector".into(),
            model_version: "1.0.0".into(),
            artifact_format: ArtifactFormat::Onnx,
            artifact_hash_algorithm: ARTIFACT_HASH_ALGORITHM.into(),
            artifact_sha256: artifact_sha256(artifact).unwrap(),
            runtime_classes: RUNTIME_CLASSES.iter().map(|v| (*v).into()).collect(),
            input_width: 640,
            input_height: 640,
            nms_embedded: true,
            release_sequence: 1,
            promotion_gate_schema: PROMOTION_GATE_SCHEMA_VERSION,
            promotion_report_sha256: "ab".repeat(32),
            created_at: "2026-07-12T00:00:00Z".into(),
            key_id: "release-2026".into(),
            signature: String::new(),
        };
        manifest.signature = BASE64.encode(key.sign(&manifest.signing_bytes().unwrap()).to_bytes());
        manifest
    }

    #[test]
    fn published_schema_matches_runtime_contract() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/model-manifest.schema.json"
        ))
        .unwrap();
        assert_eq!(
            schema["properties"]["schemaVersion"]["const"],
            MODEL_MANIFEST_SCHEMA_VERSION
        );
        let expected: Vec<serde_json::Value> = RUNTIME_CLASSES
            .iter()
            .map(|name| serde_json::json!({ "const": name }))
            .collect();
        assert_eq!(
            schema["properties"]["runtimeClasses"]["prefixItems"],
            serde_json::Value::Array(expected)
        );
    }

    #[test]
    fn published_keyring_schema_matches_runtime_contract() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/model-keyring.schema.json"
        ))
        .unwrap();
        assert_eq!(
            schema["properties"]["schemaVersion"]["const"],
            MODEL_KEYRING_SCHEMA_VERSION
        );
    }

    #[test]
    fn manifest_parser_matches_closed_published_schema() {
        let dir = tempdir().unwrap();
        let artifact = dir.path().join("model.onnx");
        fs::write(&artifact, b"model-v1").unwrap();
        let signing = SigningKey::from_bytes(&[17; 32]);
        let manifest = signed_manifest(&artifact, &signing);
        let mut value = serde_json::to_value(manifest).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unsignedExtension".into(), serde_json::json!(true));
        assert!(ModelManifest::from_json(&value.to_string()).is_err());
    }

    #[test]
    fn verifies_signature_taxonomy_and_artifact() {
        let dir = tempdir().unwrap();
        let artifact = dir.path().join("model.onnx");
        fs::write(&artifact, b"model-v1").unwrap();
        let signing = SigningKey::from_bytes(&[7; 32]);
        let manifest = signed_manifest(&artifact, &signing);
        let keys = BTreeMap::from([("release-2026".into(), signing.verifying_key())]);
        manifest.verify(&artifact, &keys).unwrap();

        fs::write(&artifact, b"tampered").unwrap();
        assert!(matches!(
            manifest.verify(&artifact, &keys),
            Err(ModelManifestError::ArtifactFingerprint)
        ));
    }

    #[test]
    fn signature_covers_contract_and_future_schema_fails_closed() {
        let dir = tempdir().unwrap();
        let artifact = dir.path().join("model.onnx");
        fs::write(&artifact, b"model-v1").unwrap();
        let signing = SigningKey::from_bytes(&[9; 32]);
        let mut manifest = signed_manifest(&artifact, &signing);
        let keys = BTreeMap::from([("release-2026".into(), signing.verifying_key())]);
        manifest.model_version = "attacker-change".into();
        assert!(matches!(
            manifest.verify(&artifact, &keys),
            Err(ModelManifestError::Signature)
        ));
        manifest.schema_version = 99;
        assert!(matches!(
            manifest.verify(&artifact, &keys),
            Err(ModelManifestError::UnsupportedSchema(99))
        ));
    }

    #[test]
    fn directory_hash_has_unambiguous_entry_boundaries() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(first.join("a"), b"Xb\0Y").unwrap();
        fs::write(second.join("a"), b"X").unwrap();
        fs::write(second.join("b"), b"Y").unwrap();
        assert_ne!(
            artifact_sha256(&first).unwrap(),
            artifact_sha256(&second).unwrap()
        );
    }

    #[test]
    fn tree_hash_matches_cross_language_fixture() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("a.txt"), b"X").unwrap();
        fs::write(dir.path().join("nested/b.bin"), b"Y").unwrap();
        assert_eq!(
            artifact_sha256(dir.path()).unwrap(),
            "8961fa489500947939aca204c59fff6f27f19e0fabb49cab47e2d0158efcf134"
        );
    }

    #[test]
    fn atomic_install_preserves_previous_artifact() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("candidate.onnx");
        let destination = dir.path().join("active.onnx");
        fs::write(&source, b"candidate").unwrap();
        fs::write(&destination, b"previous").unwrap();
        let signing = SigningKey::from_bytes(&[11; 32]);
        let manifest = signed_manifest(&source, &signing);
        let keys = BTreeMap::from([("release-2026".into(), signing.verifying_key())]);

        let backup = install_verified_artifact(&manifest, &source, &destination, &keys).unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"candidate");
        assert_eq!(fs::read(backup).unwrap(), b"previous");
    }

    #[test]
    fn installer_rejects_concurrent_lock_and_directory_artifacts() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("candidate.onnx");
        let destination = dir.path().join("active.onnx");
        fs::write(&source, b"candidate").unwrap();
        let signing = SigningKey::from_bytes(&[12; 32]);
        let manifest = signed_manifest(&source, &signing);
        let keys = BTreeMap::from([("release-2026".into(), signing.verifying_key())]);
        fs::write(dir.path().join("active.onnx.update.lock"), b"held").unwrap();
        assert!(matches!(
            install_verified_artifact(&manifest, &source, &destination, &keys),
            Err(ModelManifestError::InstallCollision(_))
        ));

        let package = dir.path().join("candidate.mlpackage");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("Manifest.json"), b"{}").unwrap();
        let package_manifest = signed_manifest(&package, &signing);
        assert!(matches!(
            install_verified_artifact(&package_manifest, &package, &destination, &keys),
            Err(ModelManifestError::InvalidField(_))
        ));
    }

    #[test]
    fn keyring_is_strict_and_release_requires_a_key() {
        let signing = SigningKey::from_bytes(&[14; 32]);
        let public_key = BASE64.encode(signing.verifying_key().to_bytes());
        let json = serde_json::json!({
            "schemaVersion": MODEL_KEYRING_SCHEMA_VERSION,
            "keys": [{"keyId": "release-2026", "publicKeyBase64": public_key}],
        });
        let keys = TrustedKeyringDocument::from_json(&json.to_string(), true).unwrap();
        assert_eq!(keys.get("release-2026"), Some(&signing.verifying_key()));

        assert!(matches!(
            TrustedKeyringDocument::from_json(r#"{"schemaVersion":1,"keys":[]}"#, true),
            Err(ModelManifestError::EmptyKeyring)
        ));
        assert!(TrustedKeyringDocument::from_json(
            r#"{"schemaVersion":1,"keys":[],"unexpected":true}"#,
            false,
        )
        .is_err());
        assert!(TrustedKeyringDocument::from_json(
            r#"{"schemaVersion":1,"schemaVersion":1,"keys":[]}"#,
            false,
        )
        .is_err());
    }

    #[test]
    fn keyring_rejects_duplicate_ids_and_bad_keys() {
        let signing = SigningKey::from_bytes(&[15; 32]);
        let public_key = BASE64.encode(signing.verifying_key().to_bytes());
        let duplicate = serde_json::json!({
            "schemaVersion": MODEL_KEYRING_SCHEMA_VERSION,
            "keys": [
                {"keyId": "same", "publicKeyBase64": public_key},
                {"keyId": "same", "publicKeyBase64": public_key},
            ],
        });
        assert!(matches!(
            TrustedKeyringDocument::from_json(&duplicate.to_string(), true),
            Err(ModelManifestError::DuplicateKey(id)) if id == "same"
        ));
        assert!(matches!(
            TrustedKeyringDocument::from_json(
                r#"{"schemaVersion":1,"keys":[{"keyId":"bad","publicKeyBase64":"AA=="}]}"#,
                true,
            ),
            Err(ModelManifestError::SignatureEncoding)
        ));
    }

    #[test]
    fn promotion_binding_and_release_sequence_are_mandatory() {
        let dir = tempdir().unwrap();
        let artifact = dir.path().join("model.onnx");
        fs::write(&artifact, b"model-v1").unwrap();
        let signing = SigningKey::from_bytes(&[16; 32]);
        let mut manifest = signed_manifest(&artifact, &signing);
        manifest.release_sequence = 0;
        assert!(matches!(
            manifest.validate_contract(),
            Err(ModelManifestError::InvalidField("releaseSequence"))
        ));
        manifest.release_sequence = 1;
        manifest.promotion_gate_schema = 4;
        assert!(matches!(
            manifest.validate_contract(),
            Err(ModelManifestError::InvalidField("promotionGateSchema"))
        ));
        manifest.promotion_gate_schema = PROMOTION_GATE_SCHEMA_VERSION;
        manifest.promotion_report_sha256 = "not-a-hash".into();
        assert!(matches!(
            manifest.validate_contract(),
            Err(ModelManifestError::InvalidField("promotionReportSha256"))
        ));
    }

    #[test]
    fn runtime_taxonomy_is_immutable() {
        let dir = tempdir().unwrap();
        let artifact = dir.path().join("model.onnx");
        fs::write(&artifact, b"model-v1").unwrap();
        let signing = SigningKey::from_bytes(&[13; 32]);
        let mut manifest = signed_manifest(&artifact, &signing);
        manifest.runtime_classes.push("Brand".into());
        assert!(matches!(
            manifest.validate_contract(),
            Err(ModelManifestError::RuntimeClasses)
        ));
    }
}
