//! Нарезка книги на фрагменты для поиска.

use crate::model::{Block, Chapter, ChunkDraft, ExtractedBook};

pub const DEFAULT_TARGET_WORDS: usize = 350;
pub const DEFAULT_OVERLAP_WORDS: usize = 50;

/// Книгу без глав длиннее этого числа слов делим на части.
const SPLIT_IF_WORDS_OVER: usize = 8000;
const PART_WORDS: usize = 5000;
const PART_PAGES: usize = 30;

fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Если у книги нет глав и она длинная, делит её на части:
/// по 30 страниц (если страницы известны) или по 5000 слов.
pub fn ensure_chapters(book: ExtractedBook) -> ExtractedBook {
    let single_untitled = book.chapters.len() == 1 && book.chapters[0].title.is_none();
    if !single_untitled {
        return book;
    }
    let total: usize = book.chapters[0].blocks.iter().map(|b| word_count(&b.text)).sum();
    if total <= SPLIT_IF_WORDS_OVER {
        return book;
    }

    let mut book = book;
    let blocks = std::mem::take(&mut book.chapters[0].blocks);
    let has_pages = blocks.iter().any(|b| b.page.is_some());
    let mut parts: Vec<Chapter> = Vec::new();
    let mut current: Vec<Block> = Vec::new();
    let mut words_in_part = 0;
    let mut pages_in_part = 0;
    let mut last_page: Option<String> = None;

    for b in blocks {
        // Новая страница?
        let new_page = has_pages && b.page.is_some() && b.page != last_page;
        let start_new = if has_pages {
            new_page && pages_in_part == PART_PAGES
        } else {
            words_in_part >= PART_WORDS
        };
        if start_new && !current.is_empty() {
            parts.push(Chapter { title: None, blocks: std::mem::take(&mut current) });
            words_in_part = 0;
            pages_in_part = 0;
        }
        if new_page {
            pages_in_part += 1;
            last_page = b.page.clone();
        }
        words_in_part += word_count(&b.text);
        current.push(b);
    }
    if !current.is_empty() {
        parts.push(Chapter { title: None, blocks: current });
    }
    book.chapters = parts;
    book
}

/// Кусочек текста, из которого собираются фрагменты (предложение или абзац).
struct Unit<'a> {
    words: Vec<&'a str>,
    page: Option<&'a str>,
    subheading: Option<&'a str>,
}

/// Делит текст на предложения (после `.!?…` и пробела).
fn split_sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for i in 0..chars.len() {
        let (pos, c) = chars[i];
        let next_is_space = chars.get(i + 1).map(|(_, n)| n.is_whitespace()).unwrap_or(false);
        if matches!(c, '.' | '!' | '?' | '…') && next_is_space {
            let end = pos + c.len_utf8();
            out.push(&text[start..end]);
            start = end;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// Режет книгу на фрагменты около `target_words` слов с перекрытием `overlap_words`.
/// Фрагмент никогда не переходит через границу главы.
pub fn chunk_book(book: &ExtractedBook, target_words: usize, overlap_words: usize) -> Vec<ChunkDraft> {
    let max_words = target_words + target_words / 5;
    let mut out = Vec::new();

    for (ci, chapter) in book.chapters.iter().enumerate() {
        // 1. Абзацы → единицы не длиннее target_words.
        let mut units: Vec<Unit> = Vec::new();
        for b in &chapter.blocks {
            let page = b.page.as_deref();
            let subheading = b.subheading.as_deref();
            let pieces = if word_count(&b.text) > target_words { split_sentences(&b.text) } else { vec![b.text.as_str()] };
            for piece in pieces {
                let words: Vec<&str> = piece.split_whitespace().collect();
                for part in words.chunks(target_words) {
                    if !part.is_empty() {
                        units.push(Unit { words: part.to_vec(), page, subheading });
                    }
                }
            }
        }

        // 2. Собираем единицы во фрагменты.
        let mut words: Vec<&str> = Vec::new(); // текст текущего фрагмента
        let mut fresh = 0; // сколько слов добавлено после перекрытия
        let mut page_start: Option<&str> = None;
        let mut page_end: Option<&str> = None;
        let mut subheading: Option<&str> = None;

        let emit = |words: &Vec<&str>, ps: Option<&str>, pe: Option<&str>, sh: Option<&str>, out: &mut Vec<ChunkDraft>| {
            out.push(ChunkDraft {
                chapter_index: ci as u32,
                subheading: sh.map(String::from),
                page_start: ps.map(String::from),
                page_end: pe.map(String::from),
                text: words.join(" "),
            });
        };

        for u in &units {
            if fresh > 0 && words.len() + u.words.len() > max_words {
                emit(&words, page_start, page_end, subheading, &mut out);
                words = words[words.len().saturating_sub(overlap_words)..].to_vec();
                fresh = 0;
                page_start = page_end;
            }
            if fresh == 0 {
                subheading = u.subheading;
                if page_start.is_none() {
                    page_start = u.page;
                }
            }
            words.extend_from_slice(&u.words);
            fresh += u.words.len();
            if u.page.is_some() {
                page_end = u.page;
            }
            if words.len() >= target_words {
                emit(&words, page_start, page_end, subheading, &mut out);
                words = words[words.len().saturating_sub(overlap_words)..].to_vec();
                fresh = 0;
                page_start = page_end;
            }
        }
        if fresh > 0 {
            emit(&words, page_start, page_end, subheading, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(prefix: &str, n: usize) -> String {
        (0..n).map(|i| format!("{prefix}{i}")).collect::<Vec<_>>().join(" ")
    }

    fn block(text: String, page: Option<&str>) -> Block {
        Block { text, page: page.map(String::from), subheading: None }
    }

    fn book(chapters: Vec<Chapter>) -> ExtractedBook {
        ExtractedBook { title: "t".into(), chapters, page_count: None }
    }

    fn count_words(s: &str) -> usize {
        s.split_whitespace().count()
    }

    #[test]
    fn ensure_chapters_splits_long_chapterless_book() {
        // 12 000 слов абзацами по 100 слов, без страниц.
        let blocks = (0..120).map(|i| block(words(&format!("w{i}_"), 100), None)).collect();
        let out = ensure_chapters(book(vec![Chapter { title: None, blocks }]));
        let sizes: Vec<usize> = out
            .chapters
            .iter()
            .map(|c| c.blocks.iter().map(|b| count_words(&b.text)).sum())
            .collect();
        assert_eq!(sizes, vec![5000, 5000, 2000]);
        assert!(out.chapters.iter().all(|c| c.title.is_none()));
    }

    #[test]
    fn ensure_chapters_splits_pdf_by_30_pages() {
        // 75 страниц по 120 слов (всего 9000 слов > 8000).
        let blocks = (1..=75)
            .map(|p| block(words(&format!("p{p}_"), 120), Some(&p.to_string())))
            .collect();
        let out = ensure_chapters(book(vec![Chapter { title: None, blocks }]));
        let starts: Vec<&str> = out
            .chapters
            .iter()
            .map(|c| c.blocks[0].page.as_deref().unwrap())
            .collect();
        assert_eq!(starts, vec!["1", "31", "61"]);
    }

    #[test]
    fn ensure_chapters_leaves_short_or_chaptered_books() {
        let short = book(vec![Chapter { title: None, blocks: vec![block(words("a", 100), None)] }]);
        assert_eq!(ensure_chapters(short.clone()), short);
        let chaptered = book(vec![
            Chapter { title: Some("1".into()), blocks: vec![block(words("a", 9000), None)] },
            Chapter { title: Some("2".into()), blocks: vec![block(words("b", 10), None)] },
        ]);
        assert_eq!(ensure_chapters(chaptered.clone()), chaptered);
    }

    #[test]
    fn chunk_never_crosses_chapter() {
        let b = book(vec![
            Chapter { title: Some("1".into()), blocks: vec![block(words("alpha", 500), None)] },
            Chapter { title: Some("2".into()), blocks: vec![block(words("beta", 500), None)] },
        ]);
        for c in chunk_book(&b, 350, 50) {
            assert!(!(c.text.contains("alpha") && c.text.contains("beta")));
            let expected = if c.text.contains("alpha") { 0 } else { 1 };
            assert_eq!(c.chapter_index, expected);
        }
    }

    #[test]
    fn chunk_sizes() {
        // Абзацы по 50 слов, всего 1000 слов; предложения по 10 слов.
        let blocks = (0..20)
            .map(|i| {
                let sentences: Vec<String> =
                    (0..5).map(|s| format!("{}.", words(&format!("b{i}s{s}w"), 10))).collect();
                block(sentences.join(" "), None)
            })
            .collect();
        let chunks = chunk_book(&book(vec![Chapter { title: None, blocks }]), 350, 50);
        assert!(chunks.len() >= 3);
        for c in &chunks {
            assert!(count_words(&c.text) <= 420, "фрагмент {} слов", count_words(&c.text));
        }
        for pair in chunks.windows(2) {
            let prev: Vec<&str> = pair[0].text.split_whitespace().collect();
            let next: Vec<&str> = pair[1].text.split_whitespace().collect();
            assert_eq!(&prev[prev.len() - 50..], &next[..50]);
        }
    }

    #[test]
    fn chunk_page_range() {
        let b = book(vec![Chapter {
            title: None,
            blocks: vec![block(words("x", 100), Some("12")), block(words("y", 100), Some("13"))],
        }]);
        let chunks = chunk_book(&b, 350, 50);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].page_start.as_deref(), Some("12"));
        assert_eq!(chunks[0].page_end.as_deref(), Some("13"));
    }

    #[test]
    fn chunk_skips_empty_chapters() {
        let b = book(vec![
            Chapter { title: Some("пусто".into()), blocks: vec![block("   ".into(), None)] },
            Chapter { title: Some("2".into()), blocks: vec![block(words("z", 20), None)] },
        ]);
        let chunks = chunk_book(&b, 350, 50);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chapter_index, 1);
        assert!(chunks.iter().all(|c| !c.text.trim().is_empty()));
    }
}
