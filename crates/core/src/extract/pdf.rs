//! Извлечение текста из PDF через pdfium: напечатанные номера страниц,
//! главы по закладкам верхнего уровня.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::*;

use super::{file_stem, ExtractError};
use crate::model::{Block, Chapter, ExtractedBook};

/// Меньше стольких букв на страницу в среднем — считаем PDF сканом.
const SCAN_CHARS_PER_PAGE: usize = 50;

/// Где искать библиотеку pdfium: переменная BOOKMIND_PDFIUM_DIR,
/// папка tools/pdfium/lib (разработка) или папка рядом с программой.
fn pdfium_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(d) = std::env::var("BOOKMIND_PDFIUM_DIR") {
        dirs.push(PathBuf::from(d));
    }
    dirs.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/pdfium/lib")));
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir);
    }
    dirs
}

/// pdfium можно подключить только один раз за запуск программы.
fn pdfium() -> Result<&'static Pdfium, ExtractError> {
    static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();
    PDFIUM
        .get_or_init(|| {
            for dir in pdfium_dirs() {
                if let Ok(b) = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&dir)) {
                    return Ok(Pdfium::new(b));
                }
            }
            Err("библиотека pdfium не найдена".to_string())
        })
        .as_ref()
        .map_err(|e| ExtractError::Corrupt(e.clone()))
}

/// Склеивает строки страницы в абзацы и убирает переносы «фото-/синтез».
fn page_paragraphs(text: &str) -> Vec<String> {
    // pdfium обозначает перенос в конце строки символом U+0002; мягкий перенос — U+00AD.
    let text = text.replace(['\u{2}', '\u{AD}'], "-\n");
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        if current.is_empty() {
            current.push_str(line);
            continue;
        }
        let ends_sentence = current.ends_with(['.', '!', '?', ':']);
        let starts_upper = line.chars().next().is_some_and(char::is_uppercase);
        let hyphen = current.ends_with('-')
            && current.chars().rev().nth(1).is_some_and(char::is_alphabetic);
        if ends_sentence && starts_upper {
            out.push(std::mem::take(&mut current));
            current.push_str(line);
        } else if hyphen {
            current.pop();
            current.push_str(line);
        } else {
            current.push(' ');
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn extract_pdf(path: &Path) -> Result<ExtractedBook, ExtractError> {
    let doc = pdfium()?
        .load_pdf_from_file(path, None)
        .map_err(|e| ExtractError::Corrupt(e.to_string()))?;

    // Главы: страницы, на которые указывают закладки верхнего уровня.
    let mut starts: Vec<(usize, String)> = Vec::new();
    if let Some(root) = doc.bookmarks().root() {
        // iter_siblings() не включает саму первую закладку.
        for b in std::iter::once(root.clone()).chain(root.iter_siblings()) {
            let page = b.destination().and_then(|d| d.page_index().ok());
            if let (Some(p), Some(title)) = (page, b.title()) {
                let entry = (p as usize, title.trim().to_string());
                if !starts.contains(&entry) {
                    starts.push(entry);
                }
            }
        }
    }
    starts.sort_by_key(|(p, _)| *p);

    let mut chapters = vec![Chapter { title: None, blocks: Vec::new() }];
    let mut letters = 0usize;
    let page_count = doc.pages().len() as usize;
    for (i, page) in doc.pages().iter().enumerate() {
        for (_, title) in starts.iter().filter(|(p, _)| *p == i) {
            chapters.push(Chapter { title: Some(title.clone()), blocks: Vec::new() });
        }
        let label = page.label().map(str::to_string).unwrap_or_else(|| (i + 1).to_string());
        let text = page.text().map(|t| t.all()).unwrap_or_default();
        letters += text.chars().filter(|c| !c.is_whitespace()).count();
        for para in page_paragraphs(&text) {
            chapters.last_mut().unwrap().blocks.push(Block { text: para, page: Some(label.clone()), subheading: None });
        }
    }

    if page_count == 0 || letters / page_count < SCAN_CHARS_PER_PAGE {
        return Err(ExtractError::ScannedPdf);
    }
    // Пустые главы (две закладки на одну страницу и т. п.) убираем.
    chapters.retain(|c| !c.blocks.is_empty());
    Ok(ExtractedBook { title: file_stem(path), chapters, page_count: Some(page_count as u32) })
}
