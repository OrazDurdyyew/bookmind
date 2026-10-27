mod common;

use bookmind_core::extract::extract_docx;
use bookmind_core::ExtractError;
use common::make_docx;

#[test]
fn headings_make_chapters_and_subheadings() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_docx(
        dir.path(),
        Some("Биология 9"),
        &[
            ("Heading1", "Глава 1. Растения"),
            ("", "Растения используют свет."),
            ("Heading2", "Фотосинтез"),
            ("", "Фотосинтез идёт в листьях."),
            ("Heading1", "Глава 2. Животные"),
            ("", "Животные двигаются."),
        ],
    );
    let book = extract_docx(&path).unwrap();
    assert_eq!(book.title, "Биология 9");
    let titles: Vec<_> = book.chapters.iter().map(|c| c.title.clone()).collect();
    assert_eq!(titles, vec![Some("Глава 1. Растения".into()), Some("Глава 2. Животные".into())]);
    let ch1 = &book.chapters[0].blocks;
    assert_eq!(ch1.len(), 2);
    assert_eq!(ch1[0].subheading, None);
    assert_eq!(ch1[1].text, "Фотосинтез идёт в листьях.");
    assert_eq!(ch1[1].subheading.as_deref(), Some("Фотосинтез"));
    assert!(book.chapters.iter().flat_map(|c| &c.blocks).all(|b| b.page.is_none()));
}

#[test]
fn title_falls_back_to_file_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_docx(dir.path(), None, &[("", "Текст.")]);
    assert_eq!(extract_docx(&path).unwrap().title, "book");
}

#[test]
fn localized_style_with_outline_level_is_a_chapter() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_docx(dir.path(), None, &[("a1", "Bap 1"), ("", "Tekst.")]);
    let book = extract_docx(&path).unwrap();
    assert_eq!(book.chapters.len(), 1);
    assert_eq!(book.chapters[0].title.as_deref(), Some("Bap 1"));
}

#[test]
fn non_zip_file_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.docx");
    std::fs::write(&path, "это не zip").unwrap();
    assert!(matches!(extract_docx(&path), Err(ExtractError::Corrupt(_))));
}

#[test]
fn extract_book_dispatches_docx() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_docx(dir.path(), None, &[("", "Текст.")]);
    assert!(bookmind_core::extract_book(&path).is_ok());
}
