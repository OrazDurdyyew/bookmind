//! Записи результатов проверки (по одной JSON-строке в файле).

use bookmind_core::quiz::QuizQuestion;
use bookmind_core::Lang;
use serde::{Deserialize, Serialize};

/// Ответ модели на вопрос из набора.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AnswerResult {
    pub id: String,
    pub lang: Lang,
    pub question: String,
    pub expected: Option<String>,
    pub answerable: bool,
    pub found: bool,
    pub answer: String,
    pub cited_pages: Vec<String>,
    pub ttft_ms: u64,
    pub tokens_per_sec: f32,
}

/// Один вопрос теста, созданный моделью.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct QuizResult {
    pub id: String,
    pub lang: Lang,
    pub book: String,
    pub question: QuizQuestion,
    pub source_text: String,
}

/// Итог генерации теста по книге: сколько просили и сколько получилось.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct QuizBatch {
    pub lang: Lang,
    pub book: String,
    pub requested: usize,
    pub generated: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Record {
    Answer(AnswerResult),
    Quiz(QuizResult),
    QuizBatch(QuizBatch),
}

impl Record {
    pub fn id(&self) -> String {
        match self {
            Record::Answer(a) => a.id.clone(),
            Record::Quiz(q) => q.id.clone(),
            Record::QuizBatch(b) => format!("batch:{}:{}", b.lang.code(), b.book),
        }
    }
}

/// Запись после оценки судьёй (или человеком).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Judged {
    pub record: Record,
    pub correct: bool,
    pub reason: String,
}

/// Читает JSONL-файл записей (пустые строки пропускаются).
pub fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &std::path::Path) -> anyhow::Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| anyhow::anyhow!("{}: {e}", path.display())))
        .collect()
}

/// Дописывает одну запись в конец JSONL-файла.
pub fn append_jsonl<T: Serialize>(path: &std::path::Path, value: &T) -> anyhow::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(value)?)?;
    Ok(())
}
