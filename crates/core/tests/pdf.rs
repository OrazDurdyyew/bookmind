use std::path::PathBuf;

use bookmind_core::extract::extract_pdf;
use bookmind_core::ExtractError;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

#[test]
fn page_count_labels_and_bookmark_chapters() {
    let book = extract_pdf(&fixture("sample.pdf")).unwrap();
    assert_eq!(book.page_count, Some(6));
    let titles: Vec<Option<&str>> = book.chapters.iter().map(|c| c.title.as_deref()).collect();
    assert_eq!(titles, vec![None, Some("Глава 1"), Some("Глава 2")]);
    // Вступление — страницы i и ii.
    let front_pages: Vec<&str> = book.chapters[0].blocks.iter().filter_map(|b| b.page.as_deref()).collect();
    assert!(front_pages.iter().all(|p| *p == "i" || *p == "ii"), "{front_pages:?}");
    // Третья страница файла напечатана как «1».
    let ch1 = &book.chapters[1].blocks;
    let first = ch1.iter().find(|b| b.text.contains("солнечный")).unwrap();
    assert_eq!(first.page.as_deref(), Some("1"));
}

#[test]
fn hyphenated_word_is_joined() {
    let book = extract_pdf(&fixture("sample.pdf")).unwrap();
    let all: Vec<&str> = book.chapters.iter().flat_map(|c| &c.blocks).map(|b| b.text.as_str()).collect();
    assert!(all.iter().any(|t| t.contains("фотосинтез")), "{all:?}");
}

#[test]
fn scanned_pdf_is_rejected() {
    assert!(matches!(extract_pdf(&fixture("scanned.pdf")), Err(ExtractError::ScannedPdf)));
}

#[test]
fn extract_book_dispatches_pdf() {
    assert!(bookmind_core::extract_book(&fixture("sample.pdf")).is_ok());
}
