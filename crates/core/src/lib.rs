//! bookmind-core — «мотор» BookMind: чтение книг, поиск, ответы и тесты.

pub mod lang;
pub mod model;

pub use lang::detect_lang;
pub use model::*;
