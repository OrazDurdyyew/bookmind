//! Набор вопросов для проверки: eval/datasets/<lang>/questions.jsonl + books/.

use std::path::{Path, PathBuf};

use bookmind_core::Lang;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EvalQuestion {
    pub id: String,
    /// Имя файла книги в папке books/.
    pub book: String,
    pub question: String,
    pub answerable: bool,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub expected_pages: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Dataset {
    pub lang: Lang,
    pub dir: PathBuf,
    pub questions: Vec<EvalQuestion>,
}

/// Загружает и проверяет набор вопросов одного языка.
pub fn load_dataset(dir: &Path, lang: Lang) -> anyhow::Result<Dataset> {
    let path = dir.join("questions.jsonl");
    let text = std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let mut questions: Vec<EvalQuestion> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let q: EvalQuestion = serde_json::from_str(line).map_err(|e| anyhow::anyhow!("{}:{}: {e}", path.display(), n + 1))?;
        if questions.iter().any(|x| x.id == q.id) {
            anyhow::bail!("повторяется id {}", q.id);
        }
        if !dir.join("books").join(&q.book).is_file() {
            anyhow::bail!("вопрос {}: нет файла книги books/{}", q.id, q.book);
        }
        if q.answerable && q.expected.as_deref().is_none_or(|e| e.trim().is_empty()) {
            anyhow::bail!("вопрос {}: answerable=true, но нет expected", q.id);
        }
        questions.push(q);
    }
    Ok(Dataset { lang, dir: dir.to_path_buf(), questions })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(lines: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("books")).unwrap();
        std::fs::write(dir.path().join("books/bio.pdf"), "x").unwrap();
        std::fs::write(dir.path().join("questions.jsonl"), lines.join("\n")).unwrap();
        dir
    }

    const OK1: &str = r#"{"id":"ru-001","book":"bio.pdf","question":"Где идёт фотосинтез?","answerable":true,"expected":"В хлоропластах","expected_pages":["87"]}"#;
    const OK2: &str = r#"{"id":"ru-002","book":"bio.pdf","question":"Кто открыл клетку?","answerable":false}"#;

    #[test]
    fn loads_valid_dataset() {
        let dir = setup(&[OK1, "", OK2]);
        let ds = load_dataset(dir.path(), Lang::Ru).unwrap();
        assert_eq!(ds.questions.len(), 2);
        assert_eq!(ds.questions[0].expected_pages, vec!["87".to_string()]);
        assert!(!ds.questions[1].answerable);
    }

    #[test]
    fn duplicate_id_is_an_error() {
        let dir = setup(&[OK1, OK1]);
        let err = load_dataset(dir.path(), Lang::Ru).unwrap_err().to_string();
        assert!(err.contains("ru-001"), "{err}");
    }

    #[test]
    fn missing_book_is_an_error() {
        let dir = setup(&[r#"{"id":"ru-003","book":"nope.epub","question":"?","answerable":false}"#]);
        let err = load_dataset(dir.path(), Lang::Ru).unwrap_err().to_string();
        assert!(err.contains("nope.epub"), "{err}");
    }

    #[test]
    fn answerable_without_expected_is_an_error() {
        let dir = setup(&[r#"{"id":"ru-004","book":"bio.pdf","question":"?","answerable":true}"#]);
        let err = load_dataset(dir.path(), Lang::Ru).unwrap_err().to_string();
        assert!(err.contains("ru-004"), "{err}");
    }
}
