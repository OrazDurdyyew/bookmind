//! Хранилище SQLite: книги, фрагменты, эмбеддинги и полнотекстовый поиск FTS5.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{BookId, ChunkDraft, ChunkId, Lang, StoredChunk};

/// Состояние книги: изучается, готова или ошибка.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookStatus {
    Indexing,
    Ready,
    Failed,
}

impl BookStatus {
    fn as_str(self) -> &'static str {
        match self {
            BookStatus::Indexing => "indexing",
            BookStatus::Ready => "ready",
            BookStatus::Failed => "failed",
        }
    }
    fn parse(s: &str) -> BookStatus {
        match s {
            "ready" => BookStatus::Ready,
            "failed" => BookStatus::Failed,
            _ => BookStatus::Indexing,
        }
    }
}

/// Строка таблицы книг.
#[derive(Clone, Debug, PartialEq)]
pub struct BookRow {
    pub id: BookId,
    pub title: String,
    pub file_name: String,
    pub lang: Lang,
    pub status: BookStatus,
    pub page_count: Option<u32>,
    pub chapter_titles: Vec<Option<String>>,
}

/// Хранилище — один файл SQLite.
pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS books (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    file_name TEXT NOT NULL,
    lang TEXT NOT NULL,
    status TEXT NOT NULL,
    page_count INTEGER,
    chapter_titles TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS chunks (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    ord INTEGER NOT NULL,
    chapter_index INTEGER NOT NULL,
    subheading TEXT,
    page_start TEXT,
    page_end TEXT,
    text TEXT NOT NULL,
    embedding BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS chunks_book ON chunks(book_id, ord);
-- remove_diacritics 2: «asgabat» находит «Aşgabat».
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    text, content='chunks', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
"#;

fn f32s_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn blob_to_f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// Превращает вопрос пользователя в безопасный запрос FTS5:
/// слова в кавычках через OR, все спецсимволы отбрасываются.
fn fts_query(query: &str) -> Option<String> {
    static WORD: OnceLock<Regex> = OnceLock::new();
    let re = WORD.get_or_init(|| Regex::new(r"[\p{L}\p{N}]+").unwrap());
    let words: Vec<String> = re
        .find_iter(query)
        .map(|m| m.as_str())
        .filter(|w| w.chars().count() >= 2)
        .map(|w| format!("\"{w}\""))
        .collect();
    (!words.is_empty()).then(|| words.join(" OR "))
}

impl Store {
    /// Открывает (или создаёт) файл базы.
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path).with_context(|| format!("не удалось открыть {}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    pub fn insert_book(
        &self,
        title: &str,
        file_name: &str,
        lang: Lang,
        page_count: Option<u32>,
        chapter_titles: &[Option<String>],
    ) -> Result<BookId> {
        self.conn.execute(
            "INSERT INTO books (title, file_name, lang, status, page_count, chapter_titles) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![title, file_name, lang.code(), BookStatus::Indexing.as_str(), page_count, serde_json::to_string(chapter_titles)?],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Сохраняет все фрагменты книги одной транзакцией.
    pub fn insert_chunks(&self, book: BookId, chunks: &[ChunkDraft], embeddings: &[Vec<f32>]) -> Result<()> {
        if chunks.len() != embeddings.len() {
            return Err(anyhow!("фрагментов {} , а эмбеддингов {}", chunks.len(), embeddings.len()));
        }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chunks (book_id, ord, chapter_index, subheading, page_start, page_end, text, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (i, (c, e)) in chunks.iter().zip(embeddings).enumerate() {
                stmt.execute(params![book, i as u32, c.chapter_index, c.subheading, c.page_start, c.page_end, c.text, f32s_to_blob(e)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_status(&self, book: BookId, status: BookStatus) -> Result<()> {
        self.conn.execute("UPDATE books SET status = ?1 WHERE id = ?2", params![status.as_str(), book])?;
        Ok(())
    }

    pub fn set_lang(&self, book: BookId, lang: Lang) -> Result<()> {
        self.conn.execute("UPDATE books SET lang = ?1 WHERE id = ?2", params![lang.code(), book])?;
        Ok(())
    }

    /// Удаляет книги, которые остались «изучаемыми» после сбоя,
    /// и возвращает имена их файлов, чтобы изучить их заново.
    pub fn remove_interrupted(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT id, file_name FROM books WHERE status = ?1 ORDER BY id")?;
        let rows: Vec<(BookId, String)> = stmt
            .query_map([BookStatus::Indexing.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, _) in &rows {
            self.delete_book(*id)?;
        }
        Ok(rows.into_iter().map(|(_, f)| f).collect())
    }

    /// Удаляет книгу; фрагменты и строки FTS удаляются каскадно.
    pub fn delete_book(&self, book: BookId) -> Result<()> {
        self.conn.execute("DELETE FROM books WHERE id = ?1", [book])?;
        Ok(())
    }

    fn row_to_book(r: &rusqlite::Row) -> rusqlite::Result<(BookId, String, String, String, String, Option<u32>, String)> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
    }

    fn to_book_row(raw: (BookId, String, String, String, String, Option<u32>, String)) -> Result<BookRow> {
        let (id, title, file_name, lang, status, page_count, titles) = raw;
        Ok(BookRow {
            id,
            title,
            file_name,
            lang: Lang::from_code(&lang).ok_or_else(|| anyhow!("неизвестный язык {lang}"))?,
            status: BookStatus::parse(&status),
            page_count,
            chapter_titles: serde_json::from_str(&titles)?,
        })
    }

    const BOOK_COLUMNS: &'static str = "id, title, file_name, lang, status, page_count, chapter_titles";

    pub fn book(&self, id: BookId) -> Result<BookRow> {
        let raw = self
            .conn
            .query_row(&format!("SELECT {} FROM books WHERE id = ?1", Self::BOOK_COLUMNS), [id], Self::row_to_book)
            .optional()?
            .ok_or_else(|| anyhow!("книга {id} не найдена"))?;
        Self::to_book_row(raw)
    }

    pub fn books(&self) -> Result<Vec<BookRow>> {
        let mut stmt = self.conn.prepare(&format!("SELECT {} FROM books ORDER BY id", Self::BOOK_COLUMNS))?;
        let raws: Vec<_> = stmt.query_map([], Self::row_to_book)?.collect::<rusqlite::Result<_>>()?;
        raws.into_iter().map(Self::to_book_row).collect()
    }

    /// Полнотекстовый поиск по книге; лучшие совпадения первыми.
    pub fn fts_search(&self, book: BookId, query: &str, k: usize) -> Result<Vec<ChunkId>> {
        let Some(q) = fts_query(query) else { return Ok(Vec::new()) };
        let mut stmt = self.conn.prepare(
            "SELECT c.id FROM chunks_fts JOIN chunks c ON c.id = chunks_fts.rowid
             WHERE chunks_fts MATCH ?1 AND c.book_id = ?2
             ORDER BY bm25(chunks_fts) LIMIT ?3",
        )?;
        let ids = stmt.query_map(params![q, book, k as i64], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    pub fn vectors(&self, book: BookId) -> Result<Vec<(ChunkId, Vec<f32>)>> {
        let mut stmt = self.conn.prepare("SELECT id, embedding FROM chunks WHERE book_id = ?1 ORDER BY ord")?;
        let rows = stmt
            .query_map([book], |r| Ok((r.get::<_, ChunkId>(0)?, blob_to_f32s(&r.get::<_, Vec<u8>>(1)?))))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Фрагменты по id — в том же порядке, что и `ids`.
    pub fn chunks(&self, ids: &[ChunkId]) -> Result<Vec<StoredChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, book_id, ord, chapter_index, subheading, page_start, page_end, text FROM chunks WHERE id = ?1",
        )?;
        let mut titles: HashMap<BookId, Vec<Option<String>>> = HashMap::new();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let mut c = stmt.query_row([id], |r| {
                Ok(StoredChunk {
                    id: r.get(0)?,
                    book_id: r.get(1)?,
                    ord: r.get(2)?,
                    chapter_index: r.get(3)?,
                    chapter_title: None,
                    subheading: r.get(4)?,
                    page_start: r.get(5)?,
                    page_end: r.get(6)?,
                    text: r.get(7)?,
                })
            })?;
            if !titles.contains_key(&c.book_id) {
                titles.insert(c.book_id, self.book(c.book_id)?.chapter_titles);
            }
            c.chapter_title = titles[&c.book_id].get(c.chapter_index as usize).cloned().flatten();
            out.push(c);
        }
        Ok(out)
    }

    /// id фрагментов по главам, в порядке книги. `None` — все главы.
    pub fn chunk_ids_by_chapter(&self, book: BookId, chapters: Option<&[u32]>) -> Result<Vec<(u32, Vec<ChunkId>)>> {
        let mut stmt = self.conn.prepare("SELECT chapter_index, id FROM chunks WHERE book_id = ?1 ORDER BY ord")?;
        let rows: Vec<(u32, ChunkId)> = stmt.query_map([book], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut out: Vec<(u32, Vec<ChunkId>)> = Vec::new();
        for (ch, id) in rows {
            if chapters.is_some_and(|wanted| !wanted.contains(&ch)) {
                continue;
            }
            match out.last_mut() {
                Some((last, ids)) if *last == ch => ids.push(id),
                _ => out.push((ch, vec![id])),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(chapter: u32, text: &str) -> ChunkDraft {
        ChunkDraft { chapter_index: chapter, subheading: None, page_start: Some("5".into()), page_end: Some("6".into()), text: text.into() }
    }

    fn store_with(texts: &[(u32, &str)]) -> (Store, BookId) {
        let s = Store::open_in_memory().unwrap();
        let b = s.insert_book("Книга", "book.pdf", Lang::Ru, Some(10), &[Some("Глава 1".into()), None, Some("Глава 3".into())]).unwrap();
        let drafts: Vec<ChunkDraft> = texts.iter().map(|(c, t)| draft(*c, t)).collect();
        let embs: Vec<Vec<f32>> = (0..drafts.len()).map(|i| vec![i as f32, 0.5, -1.25]).collect();
        s.insert_chunks(b, &drafts, &embs).unwrap();
        (s, b)
    }

    #[test]
    fn round_trip_book_chunks_and_vectors() {
        let (s, b) = store_with(&[(0, "первый"), (2, "второй")]);
        let row = s.book(b).unwrap();
        assert_eq!(row.title, "Книга");
        assert_eq!(row.lang, Lang::Ru);
        assert_eq!(row.status, BookStatus::Indexing);
        assert_eq!(row.page_count, Some(10));
        assert_eq!(row.chapter_titles, vec![Some("Глава 1".to_string()), None, Some("Глава 3".to_string())]);
        let vecs = s.vectors(b).unwrap();
        assert_eq!(vecs.len(), 2);
        assert_eq!(vecs[1].1, vec![1.0, 0.5, -1.25]);
        let ids: Vec<ChunkId> = vecs.iter().map(|(id, _)| *id).rev().collect();
        let chunks = s.chunks(&ids).unwrap();
        assert_eq!(chunks[0].text, "второй");
        assert_eq!(chunks[0].chapter_title.as_deref(), Some("Глава 3"));
        assert_eq!(chunks[0].page_start.as_deref(), Some("5"));
        assert_eq!(chunks[1].ord, 0);
    }

    #[test]
    fn delete_book_removes_chunks_and_fts() {
        let (s, b) = store_with(&[(0, "фотосинтез идёт в листьях")]);
        s.delete_book(b).unwrap();
        assert!(s.books().unwrap().is_empty());
        assert!(s.vectors(b).unwrap().is_empty());
        assert!(s.fts_search(b, "фотосинтез", 5).unwrap().is_empty());
    }

    #[test]
    fn fts_ranks_matching_chunk_first() {
        let (s, b) = store_with(&[(0, "животные двигаются"), (0, "фотосинтез идёт в листьях растений")]);
        let hits = s.fts_search(b, "где идёт фотосинтез", 5).unwrap();
        let first = s.chunks(&hits[..1]).unwrap();
        assert!(first[0].text.contains("фотосинтез"));
    }

    #[test]
    fn fts_survives_special_characters() {
        let (s, b) = store_with(&[(0, "фото синтез")]);
        assert!(s.fts_search(b, "что такое \"фото-синтез\" (OR*) it's: -x", 5).is_ok());
        assert!(s.fts_search(b, "  ?!  ", 5).unwrap().is_empty());
    }

    #[test]
    fn fts_matches_turkmen_without_special_letters() {
        let (s, b) = store_with(&[(0, "Aşgabat Türkmenistanyň paýtagtydyr")]);
        assert_eq!(s.fts_search(b, "asgabat turkmenistanyn", 5).unwrap().len(), 1);
    }

    #[test]
    fn chunk_ids_by_chapter_filters() {
        let (s, b) = store_with(&[(0, "a"), (2, "b"), (2, "c")]);
        let only2 = s.chunk_ids_by_chapter(b, Some(&[2])).unwrap();
        assert_eq!(only2.len(), 1);
        assert_eq!(only2[0].0, 2);
        assert_eq!(only2[0].1.len(), 2);
        assert_eq!(s.chunk_ids_by_chapter(b, None).unwrap().len(), 2);
    }

    #[test]
    fn remove_interrupted_and_set_lang() {
        let (s, b) = store_with(&[(0, "a")]);
        let ready = s.insert_book("Готовая", "ready.txt", Lang::En, None, &[None]).unwrap();
        s.set_status(ready, BookStatus::Ready).unwrap();
        s.set_lang(ready, Lang::Tm).unwrap();
        assert_eq!(s.remove_interrupted().unwrap(), vec!["book.pdf".to_string()]);
        let books = s.books().unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].id, ready);
        assert_eq!(books[0].lang, Lang::Tm);
        assert!(s.book(b).is_err());
    }
}
