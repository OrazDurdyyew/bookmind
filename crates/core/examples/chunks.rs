//! Печатает фрагменты книг так, как их нарезает «мотор», по одному JSON в строке.
//! Нужен, чтобы готовить примеры для дообучения модели в том же виде, что и в программе.
//! Запуск: cargo run --release -p bookmind-core --example chunks -- книга1.pdf книга2.epub … > chunks.jsonl

use bookmind_core::{chunk_book, ensure_chapters, extract_book, DEFAULT_OVERLAP_WORDS, DEFAULT_TARGET_WORDS};
use serde_json::json;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("укажите пути к книгам");
        std::process::exit(1);
    }
    for p in &paths {
        let path = std::path::Path::new(p);
        let book = match extract_book(path) {
            Ok(b) => ensure_chapters(b),
            Err(e) => {
                // Плохую книгу пропускаем, остальные обрабатываем.
                eprintln!("пропускаю {p}: {e}");
                continue;
            }
        };
        let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let chunks = chunk_book(&book, DEFAULT_TARGET_WORDS, DEFAULT_OVERLAP_WORDS);
        eprintln!("{file}: {} фрагментов", chunks.len());
        for (ord, c) in chunks.iter().enumerate() {
            let chapter_title = book.chapters.get(c.chapter_index as usize).and_then(|ch| ch.title.clone());
            let line = json!({
                "book": file, "ord": ord, "chapter_index": c.chapter_index, "chapter_title": chapter_title,
                "subheading": c.subheading, "page_start": c.page_start, "text": c.text,
            });
            println!("{line}");
        }
    }
}
