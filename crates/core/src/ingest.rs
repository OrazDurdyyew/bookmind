//! «Изучение» книги: извлечь текст → нарезать → эмбеддинги → сохранить.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::chunk::{chunk_book, ensure_chapters, DEFAULT_OVERLAP_WORDS, DEFAULT_TARGET_WORDS};
use crate::extract::{extract_book, ExtractError};
use crate::lang::detect_lang;
use crate::llm::{EmbedKind, Llm};
use crate::model::BookId;
use crate::store::{BookStatus, Store};

/// Сколько фрагментов отправлять на эмбеддинги за один запрос.
pub const EMBED_BATCH: usize = 16;
/// По скольким первым символам определяем язык книги.
const LANG_SAMPLE_CHARS: usize = 5000;

/// Этап изучения книги — для полоски прогресса.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Extracting,
    Embedding { done: usize, total: usize },
    Saving,
}

/// Флажок отмены, который можно передать в другой поток.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum IngestError {
    #[error(transparent)]
    Extract(#[from] ExtractError),
    #[error("изучение отменено")]
    Cancelled,
    #[error("ошибка модели: {0}")]
    Llm(anyhow::Error),
    #[error("ошибка базы: {0}")]
    Store(anyhow::Error),
}

/// Изучает книгу и возвращает её id. При ошибке или отмене книга удаляется,
/// чтобы в списке не осталось наполовину изученной книги.
pub async fn ingest_book<L: Llm>(
    store: &Store,
    llm: &L,
    path: &Path,
    cancel: &CancelToken,
    on_progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<BookId, IngestError> {
    on_progress(Progress::Extracting);
    let book = ensure_chapters(extract_book(path)?);
    let chunks = chunk_book(&book, DEFAULT_TARGET_WORDS, DEFAULT_OVERLAP_WORDS);

    let sample: String = book
        .chapters
        .iter()
        .flat_map(|c| c.blocks.iter())
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(LANG_SAMPLE_CHARS)
        .collect();
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let titles: Vec<Option<String>> = book.chapters.iter().map(|c| c.title.clone()).collect();
    let id = store
        .insert_book(&book.title, &file_name, detect_lang(&sample), book.page_count, &titles)
        .map_err(IngestError::Store)?;

    let result = embed_and_save(store, llm, id, &chunks, cancel, on_progress).await;
    if result.is_err() {
        // Удаляем недоделанную книгу; ошибку удаления не скрываем за исходной.
        let _ = store.delete_book(id);
    }
    result.map(|_| id)
}

async fn embed_and_save<L: Llm>(
    store: &Store,
    llm: &L,
    id: BookId,
    chunks: &[crate::model::ChunkDraft],
    cancel: &CancelToken,
    on_progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<(), IngestError> {
    let total = chunks.len();
    let mut embeddings = Vec::with_capacity(total);
    for batch in chunks.chunks(EMBED_BATCH) {
        if cancel.is_cancelled() {
            return Err(IngestError::Cancelled);
        }
        let texts: Vec<String> = batch.iter().map(|c| c.text.clone()).collect();
        let vecs = llm.embed(EmbedKind::Passage, &texts).await.map_err(IngestError::Llm)?;
        embeddings.extend(vecs);
        on_progress(Progress::Embedding { done: embeddings.len(), total });
    }
    if cancel.is_cancelled() {
        return Err(IngestError::Cancelled);
    }
    on_progress(Progress::Saving);
    store.insert_chunks(id, chunks, &embeddings).map_err(IngestError::Store)?;
    store.set_status(id, BookStatus::Ready).map_err(IngestError::Store)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeLlm;
    use crate::store::BookStatus;
    use crate::Lang;

    /// TXT на ~40 фрагментов (больше двух пачек эмбеддингов).
    fn big_txt(dir: &Path) -> std::path::PathBuf {
        let para: String = (0..350).map(|i| format!("растение{i}")).collect::<Vec<_>>().join(" ");
        let text = format!("Глава 1. Растения\n{}\n", vec![para; 40].join("\n\n"));
        let path = dir.join("big.txt");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[tokio::test]
    async fn successful_ingest_marks_ready_and_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let path = big_txt(dir.path());
        let store = Store::open_in_memory().unwrap();
        let llm = FakeLlm::new();
        let mut events = Vec::new();
        let id = ingest_book(&store, &llm, &path, &CancelToken::default(), &mut |p| events.push(p)).await.unwrap();

        let book = store.book(id).unwrap();
        assert_eq!(book.status, BookStatus::Ready);
        assert_eq!(book.lang, Lang::Ru);
        assert_eq!(book.file_name, "big.txt");
        let expected = chunk_book(&ensure_chapters(extract_book(&path).unwrap()), DEFAULT_TARGET_WORDS, DEFAULT_OVERLAP_WORDS).len();
        assert_eq!(store.vectors(id).unwrap().len(), expected);
        assert!(matches!(events.first(), Some(Progress::Extracting)));
        assert!(events.iter().any(|e| matches!(e, Progress::Embedding { done, total } if done == total && *total == expected)));
        assert!(matches!(events.last(), Some(Progress::Saving)));
    }

    #[tokio::test]
    async fn llm_failure_removes_partial_book() {
        let dir = tempfile::tempdir().unwrap();
        let path = big_txt(dir.path());
        let store = Store::open_in_memory().unwrap();
        let llm = FakeLlm { fail_embed_after: Some(1), ..FakeLlm::new() };
        let res = ingest_book(&store, &llm, &path, &CancelToken::default(), &mut |_| {}).await;
        assert!(matches!(res, Err(IngestError::Llm(_))));
        assert!(store.books().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancel_during_embedding_removes_book() {
        let dir = tempfile::tempdir().unwrap();
        let path = big_txt(dir.path());
        let store = Store::open_in_memory().unwrap();
        let llm = FakeLlm::new();
        let cancel = CancelToken::default();
        let c2 = cancel.clone();
        let res = ingest_book(&store, &llm, &path, &cancel, &mut |p| {
            if matches!(p, Progress::Embedding { .. }) {
                c2.cancel();
            }
        })
        .await;
        assert!(matches!(res, Err(IngestError::Cancelled)));
        assert!(store.books().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unsupported_format_is_an_extract_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.mobi");
        std::fs::write(&path, "x").unwrap();
        let store = Store::open_in_memory().unwrap();
        let res = ingest_book(&store, &FakeLlm::new(), &path, &CancelToken::default(), &mut |_| {}).await;
        assert!(matches!(res, Err(IngestError::Extract(ExtractError::UnsupportedFormat(_)))));
    }
}
