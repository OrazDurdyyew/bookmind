//! Общие типы данных: язык, извлечённая книга, фрагменты.

use serde::{Deserialize, Serialize};

/// Язык книги или вопроса. Туркменский — только латиница.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Tm,
    Ru,
    En,
}

impl Lang {
    /// Короткий код языка: "tm", "ru", "en".
    pub fn code(self) -> &'static str {
        match self {
            Lang::Tm => "tm",
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    /// Обратное преобразование кода в язык.
    pub fn from_code(s: &str) -> Option<Lang> {
        match s {
            "tm" => Some(Lang::Tm),
            "ru" => Some(Lang::Ru),
            "en" => Some(Lang::En),
            _ => None,
        }
    }
}

/// Книга после извлечения текста, до нарезки на фрагменты.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractedBook {
    pub title: String,
    pub chapters: Vec<Chapter>,
    /// Число страниц (есть только у PDF).
    pub page_count: Option<u32>,
}

/// Глава книги. `title: None` — глава без названия (в интерфейсе «Часть N»).
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
}

/// Абзац текста.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub text: String,
    /// Напечатанный номер страницы (только PDF).
    pub page: Option<String>,
    /// Ближайший подзаголовок внутри главы.
    pub subheading: Option<String>,
}

/// Фрагмент до сохранения в базу.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkDraft {
    pub chapter_index: u32,
    pub subheading: Option<String>,
    pub page_start: Option<String>,
    pub page_end: Option<String>,
    pub text: String,
}

pub type BookId = i64;
pub type ChunkId = i64;

/// Фрагмент, прочитанный из базы.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredChunk {
    pub id: ChunkId,
    pub book_id: BookId,
    pub ord: u32,
    pub chapter_index: u32,
    pub chapter_title: Option<String>,
    pub subheading: Option<String>,
    pub page_start: Option<String>,
    pub page_end: Option<String>,
    pub text: String,
}
