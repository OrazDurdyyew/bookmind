//! Извлечение текста из TXT (UTF-8 или Windows-1251).

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use super::{file_stem, ExtractError};
use crate::model::{Block, Chapter, ExtractedBook};

/// Строка-заголовок главы: «Глава 1», «Chapter 2», «Bap 3» (без учёта регистра).
fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(глава|chapter|bap)\s+[\p{L}\p{N}]+.*$").unwrap())
}

/// Читает TXT: сначала пробуем UTF-8, иначе считаем, что это Windows-1251.
pub fn extract_txt(path: &Path) -> Result<ExtractedBook, ExtractError> {
    let bytes = std::fs::read(path)?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(s) => s.trim_start_matches('\u{FEFF}').to_string(),
        Err(_) => encoding_rs::WINDOWS_1251.decode(&bytes).0.into_owned(),
    };

    let mut chapters = vec![Chapter { title: None, blocks: Vec::new() }];
    let mut paragraph: Vec<&str> = Vec::new();

    // Закрывает текущий абзац и добавляет его в последнюю главу.
    fn flush(paragraph: &mut Vec<&str>, chapters: &mut [Chapter]) {
        if !paragraph.is_empty() {
            let text = paragraph.join(" ");
            chapters.last_mut().unwrap().blocks.push(Block { text, page: None, subheading: None });
            paragraph.clear();
        }
    }

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut paragraph, &mut chapters);
        } else if heading_re().is_match(line) {
            flush(&mut paragraph, &mut chapters);
            chapters.push(Chapter { title: Some(line.to_string()), blocks: Vec::new() });
        } else {
            paragraph.push(line);
        }
    }
    flush(&mut paragraph, &mut chapters);

    // Пустое «вступление» без названия не нужно.
    if chapters.len() > 1 && chapters[0].blocks.is_empty() {
        chapters.remove(0);
    }
    Ok(ExtractedBook { title: file_stem(path), chapters, page_count: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    #[test]
    fn txt_detects_chapters() {
        let book = extract_txt(&fixture("sample.txt")).unwrap();
        let titles: Vec<Option<&str>> = book.chapters.iter().map(|c| c.title.as_deref()).collect();
        assert_eq!(
            titles,
            vec![None, Some("Глава 1. Растения"), Some("Chapter 2"), Some("BAP 3")]
        );
        assert_eq!(book.chapters[0].blocks[0].text, "Введение без заголовка.");
        assert_eq!(book.chapters[1].blocks.len(), 2);
        assert_eq!(book.title, "sample");
    }

    #[test]
    fn txt_cp1251_decodes() {
        let book = extract_txt(&fixture("sample-cp1251.txt")).unwrap();
        let all: String = book.chapters.iter().flat_map(|c| c.blocks.iter()).map(|b| b.text.clone()).collect();
        assert!(all.contains("фотосинтез"), "получено: {all}");
    }
}
