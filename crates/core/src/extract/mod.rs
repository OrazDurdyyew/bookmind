//! Извлечение текста из файлов книг (TXT, DOCX, EPUB, PDF).

use std::path::Path;

use crate::model::ExtractedBook;

pub mod docx;
pub mod epub;
pub mod txt;
mod xml;

pub use docx::extract_docx;
pub use epub::extract_epub;
pub use txt::extract_txt;

/// Ошибки при чтении файла книги.
#[derive(thiserror::Error, Debug)]
pub enum ExtractError {
    #[error("формат не поддерживается: {0}")]
    UnsupportedFormat(String),
    #[error("PDF является сканом (нет текстового слоя)")]
    ScannedPdf,
    #[error("файл повреждён: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Выбирает способ извлечения по расширению файла.
pub fn extract_book(path: &Path) -> Result<ExtractedBook, ExtractError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "txt" => extract_txt(path),
        "docx" => extract_docx(path),
        "epub" => extract_epub(path),
        other => Err(ExtractError::UnsupportedFormat(other.to_string())),
    }
}

/// Название книги по умолчанию — имя файла без расширения.
pub(crate) fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Без названия")
        .to_string()
}
