# BookMind: «мотор» и проверка моделей — план реализации

> **Для агентов-исполнителей:** ОБЯЗАТЕЛЬНЫЙ НАВЫК: используйте superpowers:subagent-driven-development (рекомендуется) или superpowers:executing-plans, чтобы выполнять план по задачам. Шаги отмечены чекбоксами (`- [ ]`).

**Цель:** Сделать на Rust «мотор» BookMind: извлечение текста книги → нарезка на фрагменты → гибридный поиск → ответы → генерация тестов. Это будет библиотека. К ней добавляется программа проверки (CLI), которая прогоняет модели-кандидаты через этот же «мотор» и показывает, какая самая маленькая модель проходит порог 85% на tm/ru/en.

**Архитектура:** Cargo-workspace из двух частей. `bookmind-core` — библиотека, которую потом без изменений использует приложение на Tauri. `bookmind-eval` — программа проверки. Доступ к модели идёт через трейт `Llm`. Настоящая реализация общается по HTTP с процессами `llama-server` (один для чата, один для эмбеддингов), которые `bookmind-core` сам запускает и останавливает. В тестах используется предсказуемая подделка. Всё хранится в одном файле SQLite: FTS5 для текстового поиска, эмбеддинги как BLOB, поиск по ним перебором (косинусная близость).

**Технологии:** Rust stable, tokio, reqwest, rusqlite (bundled, FTS5), zip + quick-xml (DOCX/EPUB), pdfium-render + готовая сборка pdfium (PDF), encoding_rs, regex, clap, serde/serde_json/toml, csv; готовая сборка `llama-server` из llama.cpp.

**Спецификация:** `docs/2026-10-27-bookmind-design.md`. Здесь реализуются разделы 4.2, 4.3, 4.4 и 5. Интерфейс, печать и установщик пойдут в следующих планах (см. «Дорожная карта» в конце).

## Общие ограничения

- Целевая платформа — **Windows 10/11 x64**, разработка идёт в WSL/Linux. В `bookmind-core` нельзя использовать API, которые есть только в Linux. Пути строятся через `std::path`, имена исполняемых файлов — с `std::env::consts::EXE_SUFFIX`.
- **Офлайн**: `bookmind-core` обращается по сети только к `llama-server` на `127.0.0.1`. Скрипты скачивания моделей — инструменты разработки, а не код продукта.
- Языки книг: **tm (только латиница), ru, en**.
- Ответы **только по книге**. Если ответа в книге нет, «мотор» сообщает «не найдено», а интерфейс потом покажет «В этой книге нет ответа на этот вопрос».
- Форматы: **PDF с текстовым слоем, EPUB, DOCX, TXT**. Для сканированного PDF возвращается явная ошибка `ScannedPdf`.
- Страница в ссылке — это **напечатанный номер страницы** PDF, если он есть, иначе порядковый номер страницы, начиная с 1. Для EPUB, DOCX и TXT указываются глава и подзаголовок, без страницы.
- Книгу без глав делим на части: PDF — по **30 страниц**, остальные форматы — по **5000 слов**.
- Должны работать книги до **~1000 страниц**.
- Порог качества: **≥ 85% по каждому языку** для каждой метрики (точность ответов, точность отказов, корректность вопросов теста).
- Эталонный слабый ПК для замеров скорости: **4 потока, 8 ГБ ОЗУ, без видеокарты** (уровень i5‑6500). Поэтому при проверке чат-сервер запускается с `-t 4 -ngl 0`. Цель — первый текст ответа не позже **5–10 с**.
- Автор учит Rust: **комментарии в коде на русском**, имена на английском, без хитрых макросов и сложных трюков с трейтами. Предпочитать простые функции и структуры.

## На что смотреть при проверке

Это ситуации, которые спецификация подразумевает, но которые легко упустить. Для каждой в соответствующую задачу добавлен тест.

1. Вопрос пользователя со спецсимволами FTS5 (`"`, `-`, `*`, `(`, `OR`, `'`, `:`) не должен вызывать ошибку: это просто вопрос. → тест в задаче 7.
2. Туркменский запрос без специальных букв (`asgabat`, `turkmenistanyn`) всё равно должен находить текст с `Aşgabat`, `Türkmenistanyň`. → тест в задаче 7.
3. Ответ модели в неправильном формате (`NOT_IN_BOOK` и после него текст, ссылки вроде `[9]` за пределами источников, ответ совсем без ссылок) должен давать нормальный `Answer`, а не панику. → тест в задаче 11.
4. Если `llama-server` упал посреди изучения книги, книга не должна остаться наполовину проиндексированной с пометкой «готово». Частичные записи удаляются. → тест в задаче 9.
5. TXT в кодировке Windows‑1251 (частая у старых русских текстов) декодируется правильно, без «кракозябр». → тест в задаче 3.

---

## Структура файлов

```
Cargo.toml                         workspace
.gitignore                         target/, models/, tools/llama/, tools/pdfium/, eval/out/, .superpowers/
crates/core/
  Cargo.toml
  src/lib.rs                       pub mod ...; реэкспорты
  src/model.rs                     Lang, ExtractedBook, Chapter, Block, ChunkDraft, StoredChunk, id
  src/lang.rs                      detect_lang
  src/extract/mod.rs               extract_book (выбор по расширению), ExtractError
  src/extract/txt.rs
  src/extract/docx.rs
  src/extract/epub.rs
  src/extract/pdf.rs
  src/chunk.rs                     ensure_chapters, chunk_book
  src/store.rs                     Store (схема SQLite + запросы)
  src/llm.rs                       трейт Llm, ChatRequest, Message, LlamaHttp
  src/server.rs                    управление процессом LlamaServer
  src/fake.rs                      FakeLlm (cfg(any(test, feature = "fake")))
  src/ingest.rs                    ingest_book, Progress, CancelToken
  src/retrieve.rs                  rrf, retrieve
  src/answer.rs                    build_answer_messages, parse_answer, answer
  src/quiz.rs                      plan_sources, validate, generate_one, generate_quiz
  tests/common/mod.rs              помощники make_docx, make_epub
  tests/fixtures/                  sample.txt, sample-cp1251.txt, sample.pdf, scanned.pdf
crates/eval/
  Cargo.toml
  src/main.rs                      clap: run | judge | report
  src/config.rs                    разбор models.toml
  src/dataset.rs                   загрузка questions.jsonl
  src/run.rs                       прогон кандидата → results.jsonl
  src/judge.rs                     оценка ответов и тестов → review.csv
  src/report.rs                    report.md
eval/models.toml                   кандидаты, эмбеддеры, судья
eval/datasets/{ru,en,tm}/books/    файлы книг
eval/datasets/{ru,en,tm}/questions.jsonl
eval/datasets/README.md            как составить набор проверки (на русском, для автора)
tools/fetch-llama.sh               скачивает зафиксированный релиз llama.cpp → tools/llama/
tools/fetch-pdfium.sh              скачивает сборку pdfium → tools/pdfium/
tools/fetch-models.sh              скачивает GGUF из eval/models.toml → models/
tools/make-pdf-fixtures.py         создаёт tests/fixtures/sample.pdf и scanned.pdf
```

---

### Задача 1: Инструменты, репозиторий и каркас workspace

**Файлы:**
- Создать: `Cargo.toml`, `.gitignore`, `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/eval/Cargo.toml`, `crates/eval/src/main.rs`, `tools/fetch-llama.sh`, `tools/fetch-pdfium.sh`

**Интерфейсы:**
- Даёт: workspace, который собирается; `tools/llama/llama-server` (и `tools/llama/VERSION` с тегом релиза); `tools/pdfium/lib/libpdfium.so` (для Windows позже `pdfium.dll`).

- [ ] **Шаг 1:** Если `cargo` нет, установить Rust через rustup (stable, профиль minimal). Проверка: `cargo --version` печатает версию.
- [ ] **Шаг 2:** `git init` в `/root/projects/llm`, записать `.gitignore` с путями из «Структуры файлов».
- [ ] **Шаг 3:** Создать workspace (`members = ["crates/core", "crates/eval"]`, `resolver = "2"`). `bookmind-core` — библиотека с пока пустой feature `fake`. `bookmind-eval` — бинарник, зависит от `bookmind-core` без `fake`.
- [ ] **Шаг 4:** Написать `tools/fetch-llama.sh`. Он принимает тег как `$1` (по умолчанию последний релиз из `https://api.github.com/repos/ggml-org/llama.cpp/releases/latest`), скачивает `llama-<tag>-bin-ubuntu-x64.zip`, распаковывает в `tools/llama/` и записывает тег в `tools/llama/VERSION`. Запустить. Проверка: `tools/llama/llama-server --version` печатает сборку.
- [ ] **Шаг 5:** Написать `tools/fetch-pdfium.sh`: он скачивает `pdfium-linux-x64.tgz` из последнего релиза `bblanchon/pdfium-binaries` в `tools/pdfium/`. Запустить. Проверка: `ls tools/pdfium/lib/libpdfium.so`.
- [ ] **Шаг 6:** `cargo build --workspace` — ожидается успех.
- [ ] **Шаг 7:** Коммит: `chore: workspace skeleton and dev tooling`.

### Задача 2: Доменные типы и определение языка

**Файлы:**
- Создать: `crates/core/src/model.rs`, `crates/core/src/lang.rs`

**Интерфейсы:**
- Даёт:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum Lang { Tm, Ru, En }
impl Lang { pub fn code(self) -> &'static str /* "tm"|"ru"|"en" */; pub fn from_code(s: &str) -> Option<Lang> }
pub struct ExtractedBook { pub title: String, pub chapters: Vec<Chapter>, pub page_count: Option<u32> }
pub struct Chapter { pub title: Option<String>, pub blocks: Vec<Block> }          // None = «Часть N» в интерфейсе
pub struct Block { pub text: String, pub page: Option<String>, pub subheading: Option<String> }
pub struct ChunkDraft { pub chapter_index: u32, pub subheading: Option<String>,
                        pub page_start: Option<String>, pub page_end: Option<String>, pub text: String }
pub type BookId = i64; pub type ChunkId = i64;
pub struct StoredChunk { pub id: ChunkId, pub book_id: BookId, pub ord: u32, pub chapter_index: u32,
                         pub chapter_title: Option<String>, pub subheading: Option<String>,
                         pub page_start: Option<String>, pub page_end: Option<String>, pub text: String }
pub fn detect_lang(text: &str) -> Lang
```

- [ ] **Шаг 1: Написать падающие тесты** в `lang.rs`:
```rust
assert_eq!(detect_lang("Фотосинтез — это процесс, при котором растения..."), Lang::Ru);
assert_eq!(detect_lang("Photosynthesis is the process by which plants..."), Lang::En);
assert_eq!(detect_lang("Türkmenistanyň paýtagty Aşgabat şäheridir. Ol gözel şäher."), Lang::Tm);
assert_eq!(detect_lang(""), Lang::En); // запасной вариант
```
- [ ] **Шаг 2:** `cargo test -p bookmind-core lang` → FAIL (функция не определена).
- [ ] **Шаг 3:** Реализовать. Алгоритм: посчитать буквы. Если доля кириллицы > 0.5, то `Ru`. Иначе, если доля туркменских букв `äçňöşüýž` (любой регистр) среди латинских > 0.01, то `Tm`. Иначе `En`. Вызывающий код передаёт не больше первых ~5000 символов.
- [ ] **Шаг 4:** Тесты проходят.
- [ ] **Шаг 5:** Коммит `feat(core): domain types and language detection`.

### Задача 3: Извлечение TXT и нарезка на фрагменты

**Файлы:**
- Создать: `crates/core/src/extract/mod.rs`, `crates/core/src/extract/txt.rs`, `crates/core/src/chunk.rs`, `crates/core/tests/fixtures/sample.txt`, `crates/core/tests/fixtures/sample-cp1251.txt`

**Интерфейсы:**
- Использует: типы из задачи 2.
- Даёт:
```rust
#[derive(thiserror::Error, Debug)] pub enum ExtractError { UnsupportedFormat(String), ScannedPdf, Corrupt(String), Io(#[from] std::io::Error) }
pub fn extract_book(path: &Path) -> Result<ExtractedBook, ExtractError>   // выбор по расширению в нижнем регистре
pub fn extract_txt(path: &Path) -> Result<ExtractedBook, ExtractError>
pub fn ensure_chapters(book: ExtractedBook) -> ExtractedBook
pub fn chunk_book(book: &ExtractedBook, target_words: usize, overlap_words: usize) -> Vec<ChunkDraft>
pub const DEFAULT_TARGET_WORDS: usize = 350; pub const DEFAULT_OVERLAP_WORDS: usize = 50;
```

- [ ] **Шаг 1: Написать падающие тесты:**
  - `txt_detects_chapters`: файл со строками `Глава 1. Растения`, `Chapter 2`, `BAP 3` → 3 главы с этими названиями. Текст до первого заголовка попадает в главу с `title: None`.
  - `txt_cp1251_decodes`: `sample-cp1251.txt` (создать через `iconv -t CP1251` из русского абзаца) → содержит `"фотосинтез"`.
  - `ensure_chapters_splits_long_chapterless_book`: одна глава с `title: None` на 12 000 слов, без страниц → 3 части (5000/5000/2000 слов), у всех `title: None`.
  - `ensure_chapters_splits_pdf_by_30_pages`: книга без глав, блоки на страницах "1".."75" → 3 части, начинающиеся со страниц "1", "31", "61".
  - `ensure_chapters_leaves_short_or_chaptered_books`: короткие книги и книги с главами не меняются.
  - `chunk_never_crosses_chapter`: две главы → ни один фрагмент не содержит слов из обеих.
  - `chunk_sizes`: глава на 1000 слов, target 350 / overlap 50 → каждый фрагмент ≤ 420 слов, соседние фрагменты имеют общие 50 слов.
  - `chunk_page_range`: блоки со страниц "12" и "13" в одном фрагменте → `page_start == Some("12")`, `page_end == Some("13")`.
  - `chunk_skips_empty_chapters`: пустая глава не даёт фрагментов, и нигде нет фрагментов с пустым текстом.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать.
  - TXT: прочитать байты. Если это корректный UTF‑8, убрать BOM. Иначе декодировать как Windows‑1251 через `encoding_rs`.
  - Заголовок главы — вся строка по регулярному выражению без учёта регистра: `^(глава|chapter|bap)\s+[\p{L}\p{N}]+.*$`.
  - Блоки — абзацы, разделённые пустыми строками, `page: None`. Название книги — имя файла без расширения.
  - Нарезка: набирать блоки внутри главы, пока не наберётся ≥ target слов. Блок длиннее target делить по концам предложений (`.!?…` и пробел). Следующий фрагмент начинать с последних `overlap_words` слов предыдущего. `subheading` фрагмента берётся из его первого блока.
  - `ensure_chapters` срабатывает, только если глава одна, у неё `title: None` и в ней > 8000 слов. Тогда книга делится по 30 страниц, если у блоков есть страницы, иначе по 5000 слов.
- [ ] **Шаг 4:** Тесты проходят.
- [ ] **Шаг 5:** Коммит `feat(core): txt extraction and chunking`.

### Задача 4: Извлечение DOCX

**Файлы:**
- Создать: `crates/core/src/extract/docx.rs`, `crates/core/tests/common/mod.rs`, `crates/core/tests/docx.rs`

**Интерфейсы:**
- Даёт: `pub fn extract_docx(path: &Path) -> Result<ExtractedBook, ExtractError>` и помощник для тестов `pub fn make_docx(dir: &Path, title: Option<&str>, paras: &[(&str /*styleId или ""*/, &str /*текст*/)]) -> PathBuf`. Помощник пишет минимальный корректный .docx: `[Content_Types].xml`, `word/document.xml`, `word/styles.xml` со стилями `Heading1` (outlineLvl 0) и `Heading2` (outlineLvl 1), по желанию `docProps/core.xml`.

- [ ] **Шаг 1: Написать падающие тесты:**
  - абзацы `Heading1` начинают главы;
  - `Heading2` задаёт `subheading` для следующих блоков;
  - обычные абзацы становятся блоками;
  - название берётся из `dc:title`, если оно есть, иначе из имени файла;
  - стиль с локализованным id (например `a1` с `w:outlineLvl w:val="0"` в styles.xml) тоже считается заголовком главы;
  - файл, который не является zip → `ExtractError::Corrupt`.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать через `zip` + `quick-xml`.
  - Построить из `styles.xml` таблицу styleId → уровень (`w:style/w:pPr/w:outlineLvl`). Запасной вариант: styleId `Heading{N}` → уровень N‑1.
  - Для каждого `w:p` в `document.xml` склеить текст `w:t`, прочитать `w:pStyle` и прямой `w:outlineLvl`.
  - Уровень 0 начинает новую главу, уровень ≥ 1 — подзаголовок. `page: None`.
- [ ] **Шаг 4:** Тесты проходят. Подключить `"docx"` в `extract_book`.
- [ ] **Шаг 5:** Коммит `feat(core): docx extraction`.

### Задача 5: Извлечение EPUB

**Файлы:**
- Создать: `crates/core/src/extract/epub.rs`, `crates/core/tests/epub.rs`. Изменить: `crates/core/tests/common/mod.rs`

**Интерфейсы:**
- Даёт: `pub fn extract_epub(path: &Path) -> Result<ExtractedBook, ExtractError>` и помощник `make_epub(dir, title, chapters: &[(&str /*название в оглавлении*/, &str /*тело xhtml*/)]) -> PathBuf`. Помощник создаёт EPUB 3 с `nav.xhtml` и `toc.ncx` для совместимости с EPUB 2.

- [ ] **Шаг 1: Написать падающие тесты:**
  - названия глав берутся из пунктов оглавления верхнего уровня, по порядку;
  - `<h2>`/`<h3>` внутри главы задают `subheading`;
  - `<p>` и `<li>` становятся блоками;
  - документы spine до первой цели оглавления добавляются в начало главы 1;
  - пункт оглавления, указывающий на пустой документ, не создаёт главу;
  - название из OPF `dc:title`;
  - EPUB 2 (только ncx) тоже работает.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать.
  - `META-INF/container.xml` → путь к OPF → manifest + spine.
  - Оглавление: из `nav.xhtml` (`nav[epub:type=toc] > ol > li > a`) или из `toc.ncx` (`navMap/navPoint`, верхний уровень).
  - Глава идёт от своего целевого файла до целевого файла следующего пункта в порядке spine. Якоря (`#frag`) игнорируются.
  - Текст XHTML собирается через события `quick-xml`: `script`/`style` пропускаются, сущности декодируются, пробелы схлопываются.
- [ ] **Шаг 4:** Тесты проходят. Подключить `"epub"`.
- [ ] **Шаг 5:** Коммит `feat(core): epub extraction`.

### Задача 6: Извлечение PDF

**Файлы:**
- Создать: `crates/core/src/extract/pdf.rs`, `crates/core/tests/pdf.rs`, `tools/make-pdf-fixtures.py`, `crates/core/tests/fixtures/sample.pdf`, `crates/core/tests/fixtures/scanned.pdf`

**Интерфейсы:**
- Даёт: `pub fn extract_pdf(path: &Path) -> Result<ExtractedBook, ExtractError>`. Библиотека pdfium ищется так: переменная окружения `BOOKMIND_PDFIUM_DIR`, иначе `tools/pdfium/lib` относительно workspace (разработка), иначе рядом с исполняемым файлом (продукт).

- [ ] **Шаг 1:** Написать `tools/make-pdf-fixtures.py` (запускать во временном venv с `reportlab` + `pypdf`).
  - `sample.pdf`: 6 страниц. У страниц 1–2 метки `i` и `ii`, у страниц 3–6 метки `1`..`4`.
  - Закладки верхнего уровня: «Глава 1» → страница 3, «Глава 2» → страница 5.
  - Текст на кириллице (встроить шрифт DejaVuSans). На странице 4 абзац содержит слово с переносом `фото-` / `синтез` через перенос строки.
  - `scanned.pdf`: 3 страницы, на каждой только нарисованный прямоугольник.
  - Запустить скрипт и закоммитить оба PDF.
- [ ] **Шаг 2: Написать падающие тесты:**
  - `ExtractedBook.page_count == Some(6)`;
  - главы: вступление (страницы i–ii, `title: None`), «Глава 1», «Глава 2»;
  - у блока со страницы 3 `page == Some("1")`;
  - слово с переносом извлекается как `фотосинтез`;
  - `scanned.pdf` → `Err(ExtractError::ScannedPdf)`.
- [ ] **Шаг 3:** Запустить → FAIL.
- [ ] **Шаг 4:** Реализовать через `pdfium-render`.
  - Текст по страницам. Метка страницы — через API меток страниц pdfium, иначе `(index + 1).to_string()`.
  - Главы — по страницам назначения закладок верхнего уровня (отсортированным). Если закладок нет, получается одна глава с `title: None`, и её потом поделит по 30 страниц `ensure_chapters`.
  - Абзацы: строки склеиваются, перенос `-\n` убирается. Новый блок начинается после пустой строки или после строки, которая кончается на `.!?:`, если следующая начинается с заглавной буквы.
  - Правило скана: в среднем < 50 непробельных символов на страницу → `ScannedPdf`.
- [ ] **Шаг 5:** Тесты проходят. Подключить `"pdf"`.
- [ ] **Шаг 6:** Коммит `feat(core): pdf extraction with page labels and bookmarks`.

### Задача 7: Хранилище SQLite

**Файлы:**
- Создать: `crates/core/src/store.rs`

**Интерфейсы:**
- Использует: типы из задачи 2.
- Даёт:
```rust
pub enum BookStatus { Indexing, Ready, Failed }
pub struct BookRow { pub id: BookId, pub title: String, pub file_name: String, pub lang: Lang, pub status: BookStatus, pub page_count: Option<u32>, pub chapter_titles: Vec<Option<String>> }
pub struct Store { /* rusqlite::Connection */ }
impl Store {
  pub fn open(path: &Path) -> anyhow::Result<Store>;      // создаёт схему, если её нет
  pub fn open_in_memory() -> anyhow::Result<Store>;
  pub fn insert_book(&self, title: &str, file_name: &str, lang: Lang, page_count: Option<u32>, chapter_titles: &[Option<String>]) -> anyhow::Result<BookId>;
  pub fn insert_chunks(&self, book: BookId, chunks: &[ChunkDraft], embeddings: &[Vec<f32>]) -> anyhow::Result<()>; // одна транзакция
  pub fn set_status(&self, book: BookId, status: BookStatus) -> anyhow::Result<()>;
  pub fn set_lang(&self, book: BookId, lang: Lang) -> anyhow::Result<()>;          // учитель исправляет определённый язык
  pub fn remove_interrupted(&self) -> anyhow::Result<Vec<String>>;                 // удаляет книги, оставшиеся в Indexing (приложение упало), и возвращает имена их файлов, чтобы приложение изучило их заново
  pub fn delete_book(&self, book: BookId) -> anyhow::Result<()>;   // каскадно удаляет фрагменты и строки FTS
  pub fn book(&self, id: BookId) -> anyhow::Result<BookRow>;
  pub fn books(&self) -> anyhow::Result<Vec<BookRow>>;
  pub fn fts_search(&self, book: BookId, query: &str, k: usize) -> anyhow::Result<Vec<ChunkId>>; // лучшие первыми
  pub fn vectors(&self, book: BookId) -> anyhow::Result<Vec<(ChunkId, Vec<f32>)>>;
  pub fn chunks(&self, ids: &[ChunkId]) -> anyhow::Result<Vec<StoredChunk>>;          // в том же порядке, что ids
  pub fn chunk_ids_by_chapter(&self, book: BookId, chapters: Option<&[u32]>) -> anyhow::Result<Vec<(u32, Vec<ChunkId>)>>; // None = все главы, в порядке книги
}
```

- [ ] **Шаг 1: Написать падающие тесты:**
  - книга, фрагменты и эмбеддинги сохраняются и читаются обратно (f32 little‑endian BLOB);
  - `delete_book` удаляет фрагменты и строки FTS;
  - `fts_search` ставит первым фрагмент со словами запроса;
  - **пункт проверки 1**: `fts_search(b, "что такое \"фото-синтез\" (OR*) it's: -x", 5)` возвращает `Ok`;
  - **пункт проверки 2**: для фрагмента `"Aşgabat Türkmenistanyň paýtagtydyr"` запрос `fts_search(b, "asgabat turkmenistanyn", 5)` его находит;
  - `chunk_ids_by_chapter(b, Some(&[2]))` возвращает только главу 2;
  - книга в статусе `Indexing` удаляется через `remove_interrupted`, который возвращает имя её файла, а книги `Ready` не трогаются;
  - `set_lang` сохраняется.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать через `rusqlite` (feature `bundled`).
  - Таблицы: `books`; `chunks(id, book_id, ord, chapter_index, subheading, page_start, page_end, text, embedding BLOB)`; названия глав — JSON-колонка в `books`; `chunks_fts` — таблица FTS5 с внешним содержимым по `chunks.text`, `tokenize = 'unicode61 remove_diacritics 2'`.
  - Очистка запроса: разбить запрос пользователя на слова `\p{L}\p{N}`, отбросить слова короче 2 символов, каждое взять в двойные кавычки, соединить через ` OR `. Пустой запрос → пустой список. Сортировка по `bm25`.
- [ ] **Шаг 4:** Тесты проходят.
- [ ] **Шаг 5:** Коммит `feat(core): sqlite store with fts5 and vectors`.

### Задача 8: Клиент модели, подделка и управление llama-server

**Файлы:**
- Создать: `crates/core/src/llm.rs`, `crates/core/src/fake.rs`, `crates/core/src/server.rs`

**Интерфейсы:**
- Даёт:
```rust
pub enum Role { System, User, Assistant }
pub struct Message { pub role: Role, pub content: String }
pub struct ChatRequest { pub messages: Vec<Message>, pub temperature: f32, pub max_tokens: u32 }
pub struct ChatStats { pub ttft_ms: u64, pub tokens_per_sec: f32, pub completion_tokens: u32 }
pub enum EmbedKind { Query, Passage }
pub trait Llm {
  async fn embed(&self, kind: EmbedKind, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;   // нормированы по L2
  async fn chat_stream(&self, req: &ChatRequest, on_token: &mut (dyn FnMut(&str) + Send)) -> anyhow::Result<(String, ChatStats)>;
  async fn chat_json(&self, req: &ChatRequest, schema: &serde_json::Value) -> anyhow::Result<serde_json::Value>;
}
pub struct EmbedPrefixes { pub query: String, pub passage: String }   // e5: "query: " / "passage: "; bge-m3: "" / ""
pub struct LlamaHttp { /* chat_base, embed_base, prefixes, reqwest::Client */ }
impl LlamaHttp { pub fn new(chat_base: &str, embed_base: &str, prefixes: EmbedPrefixes) -> Self }
impl Llm for LlamaHttp
pub struct FakeLlm { pub chat_replies: Mutex<VecDeque<String>>, pub json_replies: Mutex<VecDeque<serde_json::Value>>, pub fail_embed_after: Option<usize> }
impl FakeLlm { pub fn new() -> Self }   // embed предсказуемый: слова хешируются в 64-мерный «мешок слов», затем нормировка
pub struct ServerConfig { pub binary: PathBuf, pub model: PathBuf, pub port: u16, pub ctx_size: u32, pub threads: Option<u32>, pub gpu_layers: u32, pub embedding: Option<Pooling> }
pub enum Pooling { Mean, Cls }
pub struct LlamaServer { /* Child */ }
impl LlamaServer { pub async fn spawn(cfg: &ServerConfig) -> anyhow::Result<LlamaServer>; pub fn base_url(&self) -> String }  // Drop убивает процесс
```

- [ ] **Шаг 1: Написать падающие unit-тесты:**
  - `FakeLlm::embed` возвращает векторы единичной длины, одинаковые тексты дают одинаковые векторы;
  - `FakeLlm::chat_stream` выдаёт заданный ответ минимум за 2 вызова колбэка токенов и возвращает его;
  - с `fail_embed_after: Some(1)` второй вызов `embed` возвращает `Err`.
- [ ] **Шаг 2: Написать игнорируемый интеграционный тест** `tests/llama_live.rs` (`#[ignore]`).
  - Нужны переменные окружения `BOOKMIND_TEST_CHAT_MODEL` и `BOOKMIND_TEST_EMBED_MODEL` с крошечными моделями, например Qwen3‑0.6B Q4 и самый маленький эмбеддер из задачи 13.
  - Тест запускает оба сервера и проверяет:
    - `embed` возвращает векторы размерности модели;
    - `chat_stream` возвращает непустой текст и `ttft_ms > 0`;
    - `chat_json` со схемой `{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}` возвращает объект с булевым `ok`.
- [ ] **Шаг 3:** Запустить unit-тесты → FAIL.
- [ ] **Шаг 4:** Реализовать.
  - `LlamaServer::spawn`, аргументы: `-m <model> --host 127.0.0.1 --port <p> -c <ctx> --jinja`, плюс `-t <n>`, если задано, и `-ngl <gpu_layers>`. Для серверов эмбеддингов вместо `--jinja` ставится `--embedding --pooling mean|cls`.
  - Опрашивать `GET /health` каждые 250 мс до ответа 200, таймаут 120 с. Если процесс завершился раньше, сразу вернуть ошибку с хвостом его stderr.
  - `LlamaHttp`, чат: `POST /v1/chat/completions` с `stream: true` и `chat_template_kwargs: {"enable_thinking": false}` (выключает режим размышлений Qwen3), разбор SSE-строк `data:`. Статистика берётся из `timings` последнего чанка (`predicted_per_second`, `predicted_n`), TTFT замеряется на стороне клиента.
  - `chat_json`: тот же адрес без стриминга, с `response_format: {"type":"json_schema","json_schema":{"schema": <schema>}}`; `choices[0].message.content` разбирается как JSON.
  - Эмбеддинги: `POST /v1/embeddings` с `{"input": [prefix + text, ...]}`, затем нормировка по L2.
- [ ] **Шаг 5:** Unit-тесты проходят. После того как задача 13 скачает модели, запустить `cargo test -p bookmind-core --test llama_live -- --ignored` → PASS.
- [ ] **Шаг 6:** Коммит `feat(core): llm trait, llama-server client and process manager`.

### Задача 9: Конвейер «изучения» книги

**Файлы:**
- Создать: `crates/core/src/ingest.rs`

**Интерфейсы:**
- Использует: `extract_book`, `ensure_chapters`, `chunk_book`, `detect_lang`, `Store`, `Llm`.
- Даёт:
```rust
pub enum Progress { Extracting, Embedding { done: usize, total: usize }, Saving }
#[derive(Clone, Default)] pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken { pub fn cancel(&self); pub fn is_cancelled(&self) -> bool }
#[derive(thiserror::Error, Debug)] pub enum IngestError { Extract(#[from] ExtractError), Cancelled, Llm(anyhow::Error), Store(anyhow::Error) }
pub const EMBED_BATCH: usize = 16;
pub async fn ingest_book<L: Llm>(store: &Store, llm: &L, path: &Path, cancel: &CancelToken,
                                 on_progress: &mut (dyn FnMut(Progress) + Send)) -> Result<BookId, IngestError>
```

- [ ] **Шаг 1: Написать падающие тесты** (FakeLlm + `sample.txt` или сгенерированный docx):
  - успешное изучение → статус `Ready`, число фрагментов равно выводу `chunk_book`, прогресс `Embedding` доходит до `done == total`;
  - язык определяется и сохраняется;
  - **пункт проверки 4**: `FakeLlm { fail_embed_after: Some(1) }` → `Err(IngestError::Llm(_))`, и в `store.books()` этой книги больше нет;
  - отмена изнутри `on_progress` после первого события `Embedding` → `Err(Cancelled)`, книга удалена.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать.
  - Сначала вставить книгу со статусом `Indexing`.
  - Эмбеддинги фрагментов считать пачками по `EMBED_BATCH`, между пачками проверять `cancel`.
  - Все фрагменты вставить одной транзакцией, затем поставить `Ready`.
  - При любой ошибке или отмене: `delete_book` и вернуть ошибку.
  - Язык = `detect_lang` первых 5000 символов текста книги.
- [ ] **Шаг 4:** Тесты проходят.
- [ ] **Шаг 5:** Коммит `feat(core): ingest pipeline with progress and cancel`.

### Задача 10: Гибридный поиск

**Файлы:**
- Создать: `crates/core/src/retrieve.rs`

**Интерфейсы:**
- Даёт:
```rust
pub const CANDIDATES: usize = 20; pub const RRF_K: f32 = 60.0; pub const DEFAULT_TOP_K: usize = 6;
pub fn rrf(lists: &[Vec<ChunkId>], k: f32) -> Vec<ChunkId>
pub async fn retrieve<L: Llm>(store: &Store, llm: &L, book: BookId, query: &str, top_k: usize) -> anyhow::Result<Vec<StoredChunk>>
```

- [ ] **Шаг 1: Написать падающие тесты:**
  - `rrf(&[vec![1,2,3], vec![3,1]], 60.0)` → `[1, 3, 2]`;
  - id, который есть только в одном списке, всё равно попадает в результат;
  - `retrieve` с FakeLlm по изученной книге возвращает не больше `top_k` фрагментов, и первым идёт фрагмент, у которого больше всего общих слов с запросом.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать.
  - Взять топ `CANDIDATES` из FTS и топ `CANDIDATES` по векторам (скалярное произведение нормированных векторов по `store.vectors(book)`).
  - Объединить через `rrf`: оценка = Σ 1/(k + rank), rank начинается с 1, при равенстве сохраняется порядок первого появления.
  - Взять `top_k` и загрузить через `store.chunks`.
- [ ] **Шаг 4:** Тесты проходят.
- [ ] **Шаг 5:** Коммит `feat(core): hybrid retrieval with rrf`.

### Задача 11: Ответы на вопросы

**Файлы:**
- Создать: `crates/core/src/answer.rs`

**Интерфейсы:**
- Даёт:
```rust
pub const NOT_IN_BOOK: &str = "NOT_IN_BOOK";
pub struct Turn { pub question: String, pub answer: String }
pub struct Citation { pub chunk_id: ChunkId, pub chapter_title: Option<String>, pub chapter_index: u32, pub subheading: Option<String>, pub page_start: Option<String> }
pub struct Answer { pub found: bool, pub text: String, pub citations: Vec<Citation>, pub stats: ChatStats }
pub fn build_answer_messages(question: &str, history: &[Turn], sources: &[StoredChunk]) -> Vec<Message>
pub fn parse_answer(raw: &str, sources: &[StoredChunk]) -> (bool, String, Vec<Citation>)
pub async fn answer<L: Llm>(store: &Store, llm: &L, book: BookId, question: &str, history: &[Turn],
                            on_token: &mut (dyn FnMut(&str) + Send)) -> anyhow::Result<Answer>
```

- [ ] **Шаг 1: Написать падающие тесты:**
  - `build_answer_messages` кладёт в сообщение пользователя источники с номерами `[1]..[n]`, их главой и страницей, включает не больше 2 последних ходов истории, а системный промпт содержит `NOT_IN_BOOK`.
  - `parse_answer("Фотосинтез идёт в хлоропластах [1][2].", 3 источника)` → найдено, текст без меток, ссылки на источники 1 и 2.
  - **Пункт проверки 3:**
    - `"NOT_IN_BOOK"` → не найдено;
    - `"  NOT_IN_BOOK. Sorry, ..."` → не найдено;
    - `"Ответ [9] и [2]"` при 3 источниках → ссылка только на источник 2, `[9]` убрана;
    - ответ без меток → найдено, `citations` пустой;
    - повтор `[1][1]` → одна ссылка.
  - `answer` с заданным ответом FakeLlm передаёт токены через `on_token` и возвращает разобранный `Answer`.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать. Системный промпт — фиксированный текст на английском. Он специально на английском: маленькие модели лучше следуют инструкциям на английском, а язык ответа задаётся правилом.
```
You answer questions about a book using ONLY the numbered sources provided.
Rules:
- Use only facts stated in the sources. Do not use outside knowledge.
- If the sources do not contain the answer, reply with exactly: NOT_IN_BOOK
- Answer in the same language as the question.
- After each fact, cite its source number in square brackets, e.g. [2].
- Be concise: 1-5 sentences.
```
  `answer` ищет по текущему вопросу через `retrieve(.., DEFAULT_TOP_K)`, параметры `temperature 0.2`, `max_tokens 512`. Во время стриминга `on_token` для интерфейса передаётся без изменений; одинокий `NOT_IN_BOOK` приложение позже скроет само.
- [ ] **Шаг 4:** Тесты проходят.
- [ ] **Шаг 5:** Коммит `feat(core): grounded answering with citations`.

### Задача 12: Генерация тестов

**Файлы:**
- Создать: `crates/core/src/quiz.rs`

**Интерфейсы:**
- Даёт:
```rust
pub enum QuestionKind { MultipleChoice, Open, TrueFalse }
pub enum Difficulty { Easy, Medium, Hard }
pub enum CorrectAnswer { Option(usize), Bool(bool), Text(String) }
pub struct QuizQuestion { pub kind: QuestionKind, pub question: String, pub options: Vec<String>, pub correct: CorrectAnswer, pub source_chunk: ChunkId, pub page: Option<String>, pub chapter_index: u32 }
pub struct QuizRequest { pub book: BookId, pub chapters: Option<Vec<u32>>, pub kinds: Vec<QuestionKind>, pub count: usize, pub difficulty: Difficulty, pub lang: Lang }
pub fn plan_sources(by_chapter: &[(u32, Vec<ChunkId>)], count: usize) -> Vec<ChunkId>
pub fn validate(q: &QuizQuestion, earlier: &[QuizQuestion]) -> Result<(), &'static str>
pub async fn generate_one<L: Llm>(store: &Store, llm: &L, source: ChunkId, kind: QuestionKind, difficulty: Difficulty, lang: Lang, earlier: &[QuizQuestion]) -> anyhow::Result<Option<QuizQuestion>>
pub async fn generate_quiz<L: Llm>(store: &Store, llm: &L, req: &QuizRequest, cancel: &CancelToken,
                                   on_question: &mut (dyn FnMut(usize, &QuizQuestion) + Send)) -> anyhow::Result<Vec<QuizQuestion>>
```

- [ ] **Шаг 1: Написать падающие тесты:**
  - `plan_sources` для глав из 10, 10 и 10 фрагментов и count 6 → по 2 на главу, равномерно внутри главы (для 10 фрагментов — индексы 2 и 7, позиции `floor((i + 0.5) * len / per)`), главы чередуются. Если count больше общего числа фрагментов, каждый фрагмент берётся один раз, затем снова с начала.
  - `validate`:
    - вопрос с вариантами, у которого 3 варианта → Err;
    - одинаковые варианты (без учёта регистра и пробелов) → Err;
    - `Option(4)` → Err;
    - вопрос, совпадающий (после нормализации) с одним из прежних → Err;
    - да/нет с непустыми вариантами → Err;
    - хороший вопрос с вариантами → Ok.
  - `generate_quiz` с JSON-ответами FakeLlm (первый неверный, затем верный) делает повтор и возвращает `count` вопросов; типы `[MultipleChoice, TrueFalse]` чередуются; `on_question` вызывается с 1..=count.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать.
  - На каждый вопрос один вызов `chat_json` с текстом фрагмента-источника и JSON-схемой своего типа:
    - с вариантами: `question`, массив `options` с minItems=maxItems=4, `correct_index` 0–3;
    - да/нет: `question`, булев `answer`;
    - открытый: `question`, строка `answer`.
  - `temperature 0.4`. Промпт: «Write one {difficulty} {kind} question in {language name} that can be answered using ONLY this text. The correct answer must be stated in the text.»
  - На неверный ответ модели — до 2 повторов, каждый раз temperature +0.2. Если всё ещё неверно, перейти к следующему источнику из плана (при необходимости расширить план). Когда источники кончились, остановиться и вернуть то, что есть.
  - Типы вопросов идут по кругу в порядке запроса. `page` и `chapter_index` берутся из фрагмента-источника.
- [ ] **Шаг 4:** Тесты проходят.
- [ ] **Шаг 5:** Коммит `feat(core): quiz generation with validation and even spread`.

### Задача 13: Настройки проверки, скачивание моделей и формат набора вопросов

**Файлы:**
- Создать: `eval/models.toml`, `tools/fetch-models.sh`, `crates/eval/src/config.rs`, `crates/eval/src/dataset.rs`, `eval/datasets/README.md`

**Интерфейсы:**
- Даёт:
```rust
pub struct ModelEntry { pub name: String, pub url: String, pub file: String }
pub struct Candidate { pub model: ModelEntry, pub tier_hint: String, pub ctx_size: u32 }
pub struct Embedder { pub model: ModelEntry, pub pooling: Pooling, pub query_prefix: String, pub passage_prefix: String }
pub struct EvalConfig { pub candidates: Vec<Candidate>, pub embedders: Vec<Embedder>, pub judge: Candidate }
pub fn load_config(path: &Path) -> anyhow::Result<EvalConfig>
pub struct EvalQuestion { pub id: String, pub book: String, pub question: String, pub answerable: bool, pub expected: Option<String>, pub expected_pages: Vec<String> }
pub struct Dataset { pub lang: Lang, pub dir: PathBuf, pub questions: Vec<EvalQuestion> }
pub fn load_dataset(dir: &Path, lang: Lang) -> anyhow::Result<Dataset>
```

- [ ] **Шаг 1:** Заполнить `eval/models.toml`.
  - Кандидаты (GGUF Q4_K_M): Qwen3‑1.7B, Qwen3‑4B, Qwen3‑8B, Gemma‑3‑4B‑it, Gemma‑3‑12B‑it, Phi‑4‑mini‑instruct.
  - **Прежде чем писать URL, найти каждый репозиторий и файл на Hugging Face и использовать только реально существующие.** Предпочтительно официальные `Qwen/*-GGUF`, иначе `unsloth/*-GGUF`.
  - Добавить отдельным кандидатом любую более новую многоязычную instruct-модель на 3–9B, вышедшую с тех пор (например, новое поколение Qwen или Gemma).
  - Эмбеддеры: multilingual‑e5‑small и multilingual‑e5‑base (pooling mean, префиксы `query: `/`passage: `), bge‑m3 (pooling cls, без префиксов). Тоже только проверенные URL GGUF.
  - Судья — самый большой кандидат (Qwen3‑8B или новее), запускается на видеокарте.
- [ ] **Шаг 2:** Написать `tools/fetch-models.sh [name...]`: читает `models.toml` (через `python3 -c 'import tomllib'`), скачивает через `curl -L -C -` в `models/`, уже скачанные файлы пропускает.
- [ ] **Шаг 3: Написать падающие тесты** для `load_config` (пример TOML в строке) и `load_dataset`:
  - JSONL с повторяющимся id → Err с этим id;
  - вопрос со ссылкой на несуществующий файл книги → Err с именем файла;
  - `answerable: true` без `expected` → Err;
  - корректный набор → Ok со всеми вопросами.
- [ ] **Шаг 4:** Запустить → FAIL. Реализовать, тесты проходят.
- [ ] **Шаг 5:** Написать `eval/datasets/README.md` **на русском, для автора**. В нём:
  - структура папок;
  - пример строки JSONL для каждого случая: ответ есть в книге / ответа в книге нет;
  - правила: 2–3 книги и 20–30 вопросов на язык; примерно 20% вопросов, ответа на которые в книге НЕТ, но по той же теме; `expected` — короткий фактический ответ; `expected_pages` — напечатанные номера страниц; туркменский только латиницей.
- [ ] **Шаг 6:** Коммит `feat(eval): config, dataset format and model fetching`.

### Задача 14: Команды проверки `run`, `judge`, `report`

**Файлы:**
- Создать: `crates/eval/src/main.rs` (заменить заглушку), `crates/eval/src/run.rs`, `crates/eval/src/judge.rs`, `crates/eval/src/report.rs`

**Интерфейсы:**
- Использует: всё, что выше.
- Даёт команды:
  - `bookmind-eval run --candidate <name> --embedder <name> --langs ru,en,tm [--threads 4] [--quiz-per-book 10] [--dry-run]` → `eval/out/<candidate>__<embedder>/results.jsonl`. С `--dry-run` только проверяет наборы вопросов и выходит.
  - `bookmind-eval judge --dir eval/out/<run>` → добавляет `judged.jsonl` и `review.csv`.
  - `bookmind-eval report` → `eval/out/report.md` по всем папкам прогонов.
- Записи результатов (serde, по одному JSON в строке):
  - `AnswerResult { id, lang, question, expected, answerable, found, answer, cited_pages, ttft_ms, tokens_per_sec }`;
  - `QuizResult { lang, book, question: QuizQuestion, source_text }`.
  - Оценённая запись дополнительно содержит `correct: bool, reason: String`.

- [ ] **Шаг 1: Написать падающие тесты** для чистых функций:
  - `score_answer`:
    - вопроса нет в книге и `!found` → верно, судья не нужен;
    - вопроса нет в книге, а модель ответила → неверно;
    - ответ есть в книге, а `!found` → неверно;
    - ответ есть в книге и модель ответила → нужен судья.
  - `apply_overrides`: строка `review.csv` с `human_override = "1"`/`"0"` заменяет решение судьи.
  - `summarize`: по оценённым записям считает для каждого языка точность ответов (только по вопросам с ответом в книге), точность отказов (только по вопросам без ответа), корректность вопросов теста, медиану TTFT, среднюю скорость tok/s и `passed = все три метрики ≥ 0.85`.
- [ ] **Шаг 2:** Запустить → FAIL.
- [ ] **Шаг 3:** Реализовать `run`.
  - Запустить сервер эмбеддингов (`-ngl 0`) и чат-сервер (`-t <threads> -ngl 0`, ctx из настроек).
  - Каждую книгу набора проиндексировать один раз в `eval/out/index-<embedder>.sqlite` (пропускать, если файл уже есть).
  - Для каждого вопроса вызвать `answer` с пустой историей.
  - Затем `generate_quiz`: по 10 вопросов с вариантами на книгу, по всем главам.
  - Дописывать записи по мере готовности. Прогон можно продолжить после остановки: id, которые уже есть в results.jsonl, пропускаются.
- [ ] **Шаг 4:** Реализовать `judge`.
  - Сервер судьи с `-ngl 99`.
  - Ответы, которым нужен судья: `chat_json` с вопросом, эталонным ответом и ответом модели, схема `{correct: boolean, reason: string}`, инструкция «Is the model answer factually consistent with the expected answer? Minor wording differences are fine; missing the key fact is incorrect.»
  - Тест: `chat_json` с текстом источника и вопросом, схема `{correct_is_right: boolean, exactly_one_correct: boolean, answerable_from_text: boolean, reason: string}`. Вопрос корректен, если все три поля true.
  - Записать `review.csv` с колонками `id, lang, kind, question, expected, model_output, judge_verdict, reason, human_override`.
- [ ] **Шаг 5:** Реализовать `report`: таблица Markdown «прогон × язык» с метриками `summarize` и ✅/❌. В последней строке — самый маленький прошедший кандидат для каждого языка (порядок по размеру файла).
- [ ] **Шаг 6:** Тесты проходят. Коммит `feat(eval): run, judge and report commands`.

### Задача 15: Наборы вопросов ru/en и первый базовый замер

**Файлы:**
- Создать: `eval/datasets/ru/…`, `eval/datasets/en/…`, `eval/out/report.md` (копия коммитится как `eval/baseline-<дата>.md`)

- [ ] **Шаг 1:** Подобрать по 2–3 книги **в общественном достоянии** на язык, с настоящими главами и желательно учебные: например, EPUB/TXT с Project Gutenberg для en, русские тексты из Викитеки или lib.ru для ru, хотя бы один PDF на язык. Источник и лицензию каждой книги записать в `eval/datasets/<lang>/SOURCES.md`.
- [ ] **Шаг 2:** Написать по 25 вопросов на язык (20 с ответом в книге, 5 без ответа, но по теме) по правилам `eval/datasets/README.md`. Проверить: `bookmind-eval run --dry-run` → OK.
- [ ] **Шаг 3:** `tools/fetch-models.sh` для всех кандидатов, эмбеддеров и судьи.
- [ ] **Шаг 4:** Сначала выбрать эмбеддер: прогнать самого дешёвого чат-кандидата (Qwen3‑4B) с каждым эмбеддером на ru+en. Оставить лучший по точности ответов (при равенстве — меньший).
- [ ] **Шаг 5:** Прогнать всех кандидатов с выбранным эмбеддером на ru+en, затем `judge`, затем `report`.
- [ ] **Шаг 6:** Скопировать отчёт в `eval/baseline-<дата>.md` и добавить короткое резюме на русском для автора: какая самая маленькая модель проходит порог для ru/en, какая скорость на 4 потоках и что туркменский пока открыт (ждёт набора вопросов от автора). Коммит `docs(eval): first ru/en baseline`.
- [ ] **Шаг 7:** Когда автор передаст `eval/datasets/tm/`, повторить шаги 4–6 с `tm`. Таблицу уровней в спецификации (раздел 5.3) обновить по этому результату.

---

## Дорожная карта (следующие планы, пишутся после этого)

2. **Окно приложения, библиотека и чат.** Интерфейс на Tauri 2 + React/TS/Vite в стиле C (иконки Lucide), переводы tm/ru/en, sidecar `llama-server.exe` + `pdfium.dll`, команды Tauri поверх `bookmind-core`: изучение книги с прогрессом и отменой, список книг, чат со стримингом и ссылками, сохранение истории чатов.
3. **Тесты в интерфейсе и печать.** Настройки теста (галочки глав / вся книга, типы, количество, сложность), постепенное создание, редактирование, «заменить другим» (`generate_one`), сохранённые тесты, печать/PDF: лист для учеников (Имя/Класс/Дата), ключ ответов, варианты А/Б.
4. **Первый запуск и установщик.** Проверка компьютера (ОЗУ, AVX2, видеокарта/Vulkan, диск), выбор уровня, сообщение «ниже минимума», установщик NSIS с моделью «Стандарт», пакеты Pro `.bmpack` (каждый файл < 4 ГБ, при необходимости делится), сборки llama.cpp для Windows (CPU/Vulkan).
