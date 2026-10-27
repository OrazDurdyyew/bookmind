//! bookmind-core — «мотор» BookMind: чтение книг, поиск, ответы и тесты.

pub mod chunk;
pub mod extract;
pub mod lang;
pub mod model;
pub mod store;

pub use chunk::{chunk_book, ensure_chapters, DEFAULT_OVERLAP_WORDS, DEFAULT_TARGET_WORDS};
pub use extract::{extract_book, ExtractError};
pub use lang::detect_lang;
pub use model::*;
pub use store::{BookRow, BookStatus, Store};
