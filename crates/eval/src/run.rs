//! Команда `run`: прогон кандидата по наборам вопросов и генерация тестов.

use std::collections::HashSet;
use std::path::Path;

use bookmind_core::answer::answer;
use bookmind_core::ingest::{ingest_book, CancelToken};
use bookmind_core::llm::{EmbedPrefixes, LlamaHttp};
use bookmind_core::quiz::{generate_quiz, Difficulty, QuestionKind, QuizRequest};
use bookmind_core::server::{free_port, LlamaServer, ServerConfig};
use bookmind_core::{BookStatus, Lang, Store};

use crate::config::{load_config, Candidate, Embedder};
use crate::dataset::{load_dataset, Dataset};
use crate::paths;
use crate::records::{append_jsonl, read_jsonl, AnswerResult, QuizBatch, QuizResult, Record};

pub struct RunArgs {
    pub candidate: String,
    pub embedder: String,
    pub langs: Vec<Lang>,
    pub threads: u32,
    pub quiz_per_book: usize,
    pub dry_run: bool,
    /// Запустить чат-модель на видеокарте (для проверки качества; скорость тогда не показательна).
    pub gpu: bool,
    /// Не больше стольких вопросов на язык (для короткого замера скорости).
    pub limit: Option<usize>,
}

/// Книги набора без повторов, в порядке появления в вопросах.
fn dataset_books(ds: &Dataset) -> Vec<String> {
    let mut seen = Vec::new();
    for q in &ds.questions {
        if !seen.contains(&q.book) {
            seen.push(q.book.clone());
        }
    }
    seen
}

pub async fn spawn_chat(root: &Path, c: &Candidate, threads: Option<u32>) -> anyhow::Result<LlamaServer> {
    let gpu = c.gpu_layers > 0;
    LlamaServer::spawn(&ServerConfig {
        binary: paths::llama_binary(root, gpu),
        model: paths::model_path(root, &c.model.file),
        port: free_port(),
        ctx_size: c.ctx_size,
        threads,
        gpu_layers: c.gpu_layers,
        embedding: None,
    })
    .await
}

async fn spawn_embed(root: &Path, e: &Embedder) -> anyhow::Result<LlamaServer> {
    LlamaServer::spawn(&ServerConfig {
        binary: paths::llama_binary(root, false),
        model: paths::model_path(root, &e.model.file),
        port: free_port(),
        ctx_size: 2048,
        threads: Some(4),
        gpu_layers: 0,
        embedding: Some(e.pooling),
    })
    .await
}

pub async fn run(args: RunArgs) -> anyhow::Result<()> {
    let root = paths::root()?;
    let cfg = load_config(&root.join("eval/models.toml"))?;
    let mut candidate = cfg.candidate(&args.candidate)?.clone();
    if args.gpu {
        candidate.gpu_layers = 99;
    }
    let mode = if candidate.gpu_layers > 0 { "gpu" } else { "cpu" };
    let embedder = cfg.embedder(&args.embedder)?.clone();
    let datasets: Vec<Dataset> = args
        .langs
        .iter()
        .map(|l| load_dataset(&root.join("eval/datasets").join(l.code()), *l))
        .collect::<anyhow::Result<_>>()?;
    for ds in &datasets {
        println!("{}: {} вопросов, {} книг", ds.lang.code(), ds.questions.len(), dataset_books(ds).len());
    }
    if args.dry_run {
        println!("OK");
        return Ok(());
    }

    let dir = paths::run_dir(&root, &candidate.model.name, &embedder.model.name, mode);
    std::fs::create_dir_all(&dir)?;
    let results = dir.join("results.jsonl");
    let done: HashSet<String> = read_jsonl::<Record>(&results)?.iter().map(Record::id).collect();

    println!("запускаю серверы: {} + {}", candidate.model.name, embedder.model.name);
    let embed = spawn_embed(&root, &embedder).await?;
    let chat = spawn_chat(&root, &candidate, (candidate.gpu_layers == 0).then_some(args.threads)).await?;
    let llm = LlamaHttp::new(
        &chat.base_url(),
        &embed.base_url(),
        EmbedPrefixes { query: embedder.query_prefix.clone(), passage: embedder.passage_prefix.clone() },
    );

    for ds in &datasets {
        // Индекс книг — общий для всех кандидатов с этим эмбеддером.
        let store = Store::open(&paths::out_dir(&root).join(format!("index-{}-{}.sqlite", embedder.model.name, ds.lang.code())))?;
        store.remove_interrupted()?;
        for book in dataset_books(ds) {
            if !store.books()?.iter().any(|b| b.file_name == book && b.status == BookStatus::Ready) {
                println!("  изучаю {book}…");
                ingest_book(&store, &llm, &ds.dir.join("books").join(&book), &CancelToken::default(), &mut |_| {}).await?;
            }
        }
        let books = store.books()?;
        let book_id = |name: &str| books.iter().find(|b| b.file_name == name).map(|b| b.id).unwrap();

        let questions = &ds.questions[..args.limit.unwrap_or(usize::MAX).min(ds.questions.len())];
        for (i, q) in questions.iter().enumerate() {
            if done.contains(&q.id) {
                continue;
            }
            let a = match answer(&store, &llm, book_id(&q.book), &q.question, &[], &mut |_| {}).await {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("ошибка на {}: {e}\nжурнал чат-сервера:\n{}", q.id, chat.stderr_tail().join("\n"));
                    return Err(e);
                }
            };
            println!("  [{}/{}] {} found={} ttft={}ms", i + 1, questions.len(), q.id, a.found, a.stats.ttft_ms);
            append_jsonl(&results, &Record::Answer(AnswerResult {
                id: q.id.clone(),
                lang: ds.lang,
                question: q.question.clone(),
                expected: q.expected.clone(),
                answerable: q.answerable,
                found: a.found,
                answer: a.text,
                cited_pages: a.citations.iter().filter_map(|c| c.page_start.clone()).collect(),
                ttft_ms: a.stats.ttft_ms,
                tokens_per_sec: a.stats.tokens_per_sec,
            }))?;
        }

        for book in dataset_books(ds) {
            let batch_id = Record::QuizBatch(QuizBatch { lang: ds.lang, book: book.clone(), requested: 0, generated: 0 }).id();
            if done.contains(&batch_id) || args.quiz_per_book == 0 {
                continue;
            }
            println!("  тест по {book}…");
            let id = book_id(&book);
            let req = QuizRequest {
                book: id, chapters: None, kinds: vec![QuestionKind::MultipleChoice], count: args.quiz_per_book,
                difficulty: Difficulty::Medium, lang: ds.lang,
            };
            let qs = generate_quiz(&store, &llm, &req, &CancelToken::default(), &mut |n, _| println!("    вопрос {n}")).await?;
            for (n, q) in qs.iter().enumerate() {
                let source_text = store.chunks(&[q.source_chunk])?.remove(0).text;
                append_jsonl(&results, &Record::Quiz(QuizResult {
                    id: format!("quiz:{}:{}:{}", ds.lang.code(), book, n + 1),
                    lang: ds.lang, book: book.clone(), question: q.clone(), source_text,
                }))?;
            }
            append_jsonl(&results, &Record::QuizBatch(QuizBatch { lang: ds.lang, book: book.clone(), requested: args.quiz_per_book, generated: qs.len() }))?;
        }
    }
    println!("готово: {}", results.display());
    Ok(())
}
