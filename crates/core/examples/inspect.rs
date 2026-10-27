//! Показывает, как «мотор» видит книгу: главы, страницы, число фрагментов.
//! Запуск: cargo run -p bookmind-core --example inspect -- путь/к/книге [номер главы | dump]
//! `dump` печатает всю книгу как текст: «## Название главы», затем абзацы.

use bookmind_core::{chunk_book, detect_lang, ensure_chapters, extract_book, DEFAULT_OVERLAP_WORDS, DEFAULT_TARGET_WORDS};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(args.get(1).expect("укажите путь к книге"));
    let book = match extract_book(path) {
        Ok(b) => ensure_chapters(b),
        Err(e) => {
            eprintln!("ошибка: {e}");
            std::process::exit(1);
        }
    };
    let all: String = book.chapters.iter().flat_map(|c| &c.blocks).map(|b| b.text.as_str()).collect::<Vec<_>>().join(" ");
    let chunks = chunk_book(&book, DEFAULT_TARGET_WORDS, DEFAULT_OVERLAP_WORDS);
    if args.get(2).map(String::as_str) == Some("dump") {
        for c in &book.chapters {
            println!("## {}\n", c.title.as_deref().unwrap_or("—"));
            for b in &c.blocks {
                println!("{}\n", b.text);
            }
        }
        return;
    }
    println!("«{}»: язык {:?}, страниц {:?}, глав {}, слов {}, фрагментов {}",
        book.title, detect_lang(&all.chars().take(5000).collect::<String>()), book.page_count,
        book.chapters.len(), all.split_whitespace().count(), chunks.len());
    match args.get(2).and_then(|n| n.parse::<usize>().ok()) {
        Some(n) => {
            let c = &book.chapters[n];
            println!("== [{n}] {:?}", c.title);
            for b in &c.blocks {
                println!("[стр. {:?} | {:?}] {}", b.page, b.subheading, b.text);
            }
        }
        None => {
            for (i, c) in book.chapters.iter().enumerate() {
                let words: usize = c.blocks.iter().map(|b| b.text.split_whitespace().count()).sum();
                let first_page = c.blocks.first().and_then(|b| b.page.clone());
                println!("[{i}] {:?} — {words} слов, с. {:?}", c.title, first_page);
            }
        }
    }
}
