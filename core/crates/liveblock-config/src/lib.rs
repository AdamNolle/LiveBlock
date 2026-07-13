pub mod desktop_contract;
pub mod embeddings;
pub mod model_manifest;
pub mod model_update;
pub mod settings;
pub mod vocabulary;

pub use desktop_contract::{
    developer_training_runtime_available, DesktopAction, DesktopBehaviorContract,
    DesktopCapabilityProfile, DesktopPlatform, HotkeyContract, SupportMode,
    DESKTOP_CONTRACT_VERSION,
};
pub use embeddings::ClassEmbeddings;
pub use model_manifest::{
    artifact_sha256, install_verified_artifact, verifying_key_from_base64, ArtifactFormat,
    ModelManifest, ModelManifestError, TrustedKeyring, TrustedKeyringDocument, TrustedPublicKey,
};
pub use model_update::{
    apply_verified_file_update, recover_verified_active_manifest, ModelUpdateError,
    ModelUpdateReceipt, ModelUpdateState, MODEL_UPDATE_STATE_SCHEMA_VERSION,
};
pub use settings::{ClassRule, CoordinatorConfig, DetectionSettings, SettingsStore};
pub use vocabulary::{VocabClass, Vocabulary};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(String),
    #[error("vocab version mismatch: settings {settings} vs vocabulary {vocab}")]
    VocabVersion { settings: u32, vocab: u32 },
    #[error("embedding version mismatch: {emb} vs vocabulary {vocab}")]
    EmbeddingVersion { emb: u32, vocab: u32 },
    #[error("unsupported {document} schema version {found}; current version is {current}")]
    UnsupportedSchema {
        document: &'static str,
        found: u32,
        current: u32,
    },
}

/// 2-space, sorted-key pretty JSON identical to Swift JSONEncoder([.prettyPrinted,.sortedKeys]).
pub fn pretty_two_space(value: &serde_json::Value) -> String {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    serde::Serialize::serialize(value, &mut ser).expect("serialize value");
    String::from_utf8(buf).expect("utf8")
}
