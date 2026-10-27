//! bookmind-eval — программа проверки качества моделей.
//!
//! Команды:
//!   run    — прогнать кандидата по наборам вопросов
//!   judge  — оценить ответы моделью-судьёй
//!   report — собрать отчёт eval/out/report.md

mod config;
mod dataset;
mod judge;
mod paths;
mod records;
mod report;
mod run;

use std::path::PathBuf;

use bookmind_core::Lang;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bookmind-eval", about = "Проверка качества моделей BookMind")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Прогнать кандидата по наборам вопросов.
    Run {
        #[arg(long)]
        candidate: String,
        #[arg(long)]
        embedder: String,
        /// Языки через запятую: ru,en,tm
        #[arg(long, default_value = "ru,en")]
        langs: String,
        #[arg(long, default_value_t = 4)]
        threads: u32,
        #[arg(long, default_value_t = 10)]
        quiz_per_book: usize,
        /// Только проверить наборы вопросов.
        #[arg(long)]
        dry_run: bool,
        /// Чат-модель на видеокарте (проверка качества).
        #[arg(long)]
        gpu: bool,
        /// Не больше N вопросов на язык (замер скорости).
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Оценить прогон моделью-судьёй.
    Judge {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Собрать отчёт по всем прогонам.
    Report,
}

fn parse_langs(s: &str) -> anyhow::Result<Vec<Lang>> {
    s.split(',').map(|x| Lang::from_code(x.trim()).ok_or_else(|| anyhow::anyhow!("неизвестный язык: {x}"))).collect()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Run { candidate, embedder, langs, threads, quiz_per_book, dry_run, gpu, limit } => {
            run::run(run::RunArgs { candidate, embedder, langs: parse_langs(&langs)?, threads, quiz_per_book, dry_run, gpu, limit }).await
        }
        Cmd::Judge { dir } => judge::judge(&dir).await,
        Cmd::Report => report::report(),
    }
}
