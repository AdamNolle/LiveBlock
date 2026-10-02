use serde::{Deserialize, Serialize};

use crate::{pretty_two_space, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocabClass {
    pub id: u32,
    pub name: String,
    pub prompts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vocabulary {
    pub version: u32,
    pub classes: Vec<VocabClass>,
}

impl Vocabulary {
    pub fn to_json(&self) -> String {
        // serde_json::Value sorts object keys via BTreeMap when the "preserve_order"
        // feature is OFF (the default), giving sorted keys for free.
        let value = serde_json::to_value(self).expect("to_value");
        pretty_two_space(&value)
    }

    pub fn from_json(s: &str) -> Result<Self, ConfigError> {
        Ok(serde_json::from_str(s)?)
    }

    pub fn class_name(&self, id: u32) -> Option<&str> {
        self.classes
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.as_str())
    }

    /// Dense class-name lookup indexed by model class id. Gaps remain empty so
    /// an out-of-order vocabulary can never shift model output semantics.
    pub fn class_names_by_id(&self) -> Vec<String> {
        let Some(max_id) = self.classes.iter().map(|class| class.id).max() else {
            return Vec::new();
        };
        let mut names = vec![String::new(); max_id as usize + 1];
        for class in &self.classes {
            names[class.id as usize] = class.name.clone();
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vocabulary_json_is_sorted_two_space() {
        let v = Vocabulary {
            version: 1,
            classes: vec![VocabClass {
                id: 0,
                name: "Logo".into(),
                prompts: vec!["logo".into(), "brand logo".into()],
            }],
        };
        let json = v.to_json();
        // sorted keys: classes < version; class keys: id < name < prompts
        assert!(json.starts_with("{\n  \"classes\": [\n"));
        assert!(
            json.contains("\n      \"id\": 0,\n      \"name\": \"Logo\",\n      \"prompts\": [")
        );
        assert!(json.trim_end().ends_with("\"version\": 1\n}"));
        // round-trips
        let back = Vocabulary::from_json(&json).unwrap();
        assert_eq!(back.classes[0].prompts.len(), 2);
    }

    #[test]
    fn dense_class_names_preserve_id_gaps() {
        let vocabulary = Vocabulary {
            version: 1,
            classes: vec![
                VocabClass {
                    id: 2,
                    name: "Sponsored".into(),
                    prompts: vec![],
                },
                VocabClass {
                    id: 0,
                    name: "Logo".into(),
                    prompts: vec![],
                },
            ],
        };
        assert_eq!(
            vocabulary.class_names_by_id(),
            vec!["Logo".to_string(), String::new(), "Sponsored".to_string()]
        );
    }

    #[test]
    fn default_vocab_parses() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tools/vocab/liveblock-vocab.json"
        );
        let json = std::fs::read_to_string(path).expect("read default vocab");
        let vocab = Vocabulary::from_json(&json).expect("parse default vocab");
        assert_eq!(vocab.version, 1);
        assert_eq!(vocab.classes.len(), 3);
    }
}
