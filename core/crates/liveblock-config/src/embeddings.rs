use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{pretty_two_space, vocabulary::Vocabulary, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassEmbeddings {
    pub vocab_version: u32,
    pub dim: u32,
    pub model_tag: String,
    pub vectors: BTreeMap<u32, Vec<f32>>,
}

impl ClassEmbeddings {
    pub fn to_json(&self) -> String {
        pretty_two_space(&serde_json::to_value(self).expect("to_value"))
    }

    pub fn from_json(s: &str) -> Result<Self, ConfigError> {
        Ok(serde_json::from_str(s)?)
    }

    pub fn validate_against(&self, vocab: &Vocabulary) -> Result<(), ConfigError> {
        if self.vocab_version != vocab.version {
            return Err(ConfigError::EmbeddingVersion {
                emb: self.vocab_version,
                vocab: vocab.version,
            });
        }
        for c in &vocab.classes {
            match self.vectors.get(&c.id) {
                Some(v) if v.len() as u32 == self.dim => {}
                _ => {
                    return Err(ConfigError::Io(format!(
                        "missing/mis-sized embedding for class {}",
                        c.id
                    )))
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocabulary::*;

    #[test]
    fn embeddings_reject_version_mismatch() {
        let mut vectors = std::collections::BTreeMap::new();
        vectors.insert(0u32, vec![0.1f32, 0.2, 0.3]);
        let emb = ClassEmbeddings {
            vocab_version: 2,
            dim: 3,
            model_tag: "clip-vit-b32".into(),
            vectors,
        };
        let vocab = Vocabulary {
            version: 1,
            classes: vec![VocabClass {
                id: 0,
                name: "Logo".into(),
                prompts: vec!["logo".into()],
            }],
        };
        assert!(matches!(
            emb.validate_against(&vocab),
            Err(ConfigError::EmbeddingVersion { .. })
        ));
    }

    #[test]
    fn embeddings_accept_matching() {
        let mut vectors = std::collections::BTreeMap::new();
        vectors.insert(0u32, vec![0.1f32, 0.2, 0.3]);
        let emb = ClassEmbeddings {
            vocab_version: 1,
            dim: 3,
            model_tag: "clip-vit-b32".into(),
            vectors,
        };
        let vocab = Vocabulary {
            version: 1,
            classes: vec![VocabClass {
                id: 0,
                name: "Logo".into(),
                prompts: vec!["logo".into()],
            }],
        };
        assert!(emb.validate_against(&vocab).is_ok());
    }
}
