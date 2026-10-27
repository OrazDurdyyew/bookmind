//! Настройки проверки: eval/models.toml.

use std::path::Path;

use bookmind_core::server::Pooling;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ModelEntry {
    pub name: String,
    pub url: String,
    pub file: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Candidate {
    #[serde(flatten)]
    pub model: ModelEntry,
    pub tier_hint: String,
    pub ctx_size: u32,
    #[serde(default)]
    pub gpu_layers: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Embedder {
    #[serde(flatten)]
    pub model: ModelEntry,
    pub pooling: Pooling,
    pub query_prefix: String,
    pub passage_prefix: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct EvalConfig {
    pub candidates: Vec<Candidate>,
    pub embedders: Vec<Embedder>,
    pub judge: Candidate,
}

pub fn load_config(path: &Path) -> anyhow::Result<EvalConfig> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok(toml::from_str(&text)?)
}

impl EvalConfig {
    pub fn candidate(&self, name: &str) -> anyhow::Result<&Candidate> {
        self.candidates.iter().find(|c| c.model.name == name).ok_or_else(|| anyhow::anyhow!("нет кандидата {name}"))
    }
    pub fn embedder(&self, name: &str) -> anyhow::Result<&Embedder> {
        self.embedders.iter().find(|e| e.model.name == name).ok_or_else(|| anyhow::anyhow!("нет эмбеддера {name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[[candidates]]
name = "qwen3-4b"
url = "https://example/q.gguf"
file = "q.gguf"
tier_hint = "standard"
ctx_size = 8192

[[embedders]]
name = "e5-small"
url = "https://example/e.gguf"
file = "e.gguf"
pooling = "mean"
query_prefix = "query: "
passage_prefix = "passage: "

[judge]
name = "qwen3-8b"
url = "https://example/j.gguf"
file = "j.gguf"
tier_hint = "judge"
ctx_size = 8192
gpu_layers = 99
"#;

    #[test]
    fn parses_sample_config() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("models.toml");
        std::fs::write(&p, SAMPLE).unwrap();
        let cfg = load_config(&p).unwrap();
        assert_eq!(cfg.candidates[0].model.name, "qwen3-4b");
        assert_eq!(cfg.candidates[0].gpu_layers, 0);
        assert_eq!(cfg.embedders[0].pooling, Pooling::Mean);
        assert_eq!(cfg.embedders[0].query_prefix, "query: ");
        assert_eq!(cfg.judge.gpu_layers, 99);
    }

    #[test]
    fn real_models_toml_parses() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../eval/models.toml");
        let cfg = load_config(&p).unwrap();
        assert!(cfg.candidates.len() >= 6);
        assert!(cfg.embedders.iter().any(|e| e.model.name == "bge-m3" && e.pooling == Pooling::Cls));
    }
}
