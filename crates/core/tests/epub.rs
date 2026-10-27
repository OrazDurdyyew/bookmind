mod common;

use bookmind_core::extract::extract_epub;
use common::{make_epub, make_epub_with, EpubDoc};

fn titles(book: &bookmind_core::ExtractedBook) -> Vec<Option<String>> {
    book.chapters.iter().map(|c| c.title.clone()).collect()
}

#[test]
fn chapters_come_from_toc_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_epub(dir.path(), "Biology", &[("Chapter 1", "<p>Cells.</p>"), ("Chapter 2", "<p>Plants.</p>")]);
    let book = extract_epub(&path).unwrap();
    assert_eq!(book.title, "Biology");
    assert_eq!(titles(&book), vec![Some("Chapter 1".into()), Some("Chapter 2".into())]);
    assert_eq!(book.chapters[1].blocks[0].text, "Plants.");
}

#[test]
fn headings_set_subheading_and_list_items_are_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let body = "<h1>Chapter 1</h1><p>Intro &amp; more.</p><h2>Photosynthesis</h2><p>Light   is\n used.</p><ul><li>Leaves</li></ul><script>var x;</script>";
    let path = make_epub(dir.path(), "B", &[("Chapter 1", body)]);
    let blocks = &extract_epub(&path).unwrap().chapters[0].blocks;
    let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, vec!["Intro & more.", "Light is used.", "Leaves"]);
    assert_eq!(blocks[0].subheading, None);
    assert_eq!(blocks[1].subheading.as_deref(), Some("Photosynthesis"));
    assert_eq!(blocks[2].subheading.as_deref(), Some("Photosynthesis"));
}

#[test]
fn documents_before_first_toc_entry_go_to_chapter_one() {
    let dir = tempfile::tempdir().unwrap();
    let docs = [
        EpubDoc { toc_title: None, body: "<p>Preface.</p>" },
        EpubDoc { toc_title: Some("Chapter 1"), body: "<p>Body.</p>" },
    ];
    let book = extract_epub(&make_epub_with(dir.path(), "B", &docs, true)).unwrap();
    assert_eq!(titles(&book), vec![Some("Chapter 1".into())]);
    let texts: Vec<&str> = book.chapters[0].blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, vec!["Preface.", "Body."]);
}

#[test]
fn toc_entry_with_empty_document_makes_no_chapter() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_epub(dir.path(), "B", &[("Cover", ""), ("Chapter 1", "<p>Body.</p>")]);
    assert_eq!(titles(&extract_epub(&path).unwrap()), vec![Some("Chapter 1".into())]);
}

#[test]
fn epub2_with_ncx_only_works() {
    let dir = tempfile::tempdir().unwrap();
    let docs = [
        EpubDoc { toc_title: Some("Bap 1"), body: "<p>Tekst.</p>" },
        EpubDoc { toc_title: Some("Bap 2"), body: "<p>Başga.</p>" },
    ];
    let book = extract_epub(&make_epub_with(dir.path(), "Kitap", &docs, false)).unwrap();
    assert_eq!(titles(&book), vec![Some("Bap 1".into()), Some("Bap 2".into())]);
}

#[test]
fn extract_book_dispatches_epub() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_epub(dir.path(), "B", &[("C", "<p>x</p>")]);
    assert!(bookmind_core::extract_book(&path).is_ok());
}
