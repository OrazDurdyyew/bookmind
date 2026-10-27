//! Запуск и остановка процесса llama-server.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// Как сервер эмбеддингов собирает вектор текста: среднее (e5) или CLS (bge-m3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pooling {
    Mean,
    Cls,
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub port: u16,
    pub ctx_size: u32,
    /// Число потоков процессора; None — пусть решает llama.cpp.
    pub threads: Option<u32>,
    /// Сколько слоёв модели отдать видеокарте (0 — только процессор).
    pub gpu_layers: u32,
    /// Some — сервер эмбеддингов, None — чат-сервер.
    pub embedding: Option<Pooling>,
}

/// Запущенный llama-server. При удалении объекта процесс завершается.
pub struct LlamaServer {
    _child: Child,
    port: u16,
}

const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);
const STDERR_TAIL_LINES: usize = 20;

/// Свободный порт на 127.0.0.1.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).unwrap_or(0)
}

impl LlamaServer {
    /// Запускает сервер и ждёт, пока он ответит на /health.
    pub async fn spawn(cfg: &ServerConfig) -> anyhow::Result<LlamaServer> {
        let mut cmd = Command::new(&cfg.binary);
        cmd.arg("-m").arg(&cfg.model)
            .args(["--host", "127.0.0.1", "--port", &cfg.port.to_string(), "-c", &cfg.ctx_size.to_string()])
            .args(["-ngl", &cfg.gpu_layers.to_string()]);
        if let Some(t) = cfg.threads {
            cmd.args(["-t", &t.to_string()]);
        }
        match cfg.embedding {
            Some(p) => {
                cmd.args(["--embedding", "--pooling", if p == Pooling::Mean { "mean" } else { "cls" }]);
            }
            None => {
                cmd.arg("--jinja");
            }
        }
        cmd.stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: без чёрного окна консоли

        let mut child = cmd.spawn().map_err(|e| anyhow::anyhow!("не удалось запустить {}: {e}", cfg.binary.display()))?;

        // Последние строки stderr — чтобы показать причину, если сервер упадёт.
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        if let Some(stderr) = child.stderr.take() {
            let tail = tail.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut t = tail.lock().unwrap();
                    if t.len() == STDERR_TAIL_LINES {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            });
        }

        let url = format!("http://127.0.0.1:{}/health", cfg.port);
        let http = reqwest::Client::new();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                tokio::time::sleep(Duration::from_millis(100)).await; // дочитать stderr
                let tail: Vec<String> = tail.lock().unwrap().iter().cloned().collect();
                anyhow::bail!("llama-server завершился ({status}):\n{}", tail.join("\n"));
            }
            if let Ok(r) = http.get(&url).send().await {
                if r.status().is_success() {
                    return Ok(LlamaServer { _child: child, port: cfg.port });
                }
            }
            if started.elapsed() > HEALTH_TIMEOUT {
                anyhow::bail!("llama-server не ответил за {} с", HEALTH_TIMEOUT.as_secs());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_fails_fast_when_model_is_missing() {
        let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/llama/llama-server");
        let cfg = ServerConfig {
            binary, model: "/nonexistent/model.gguf".into(), port: free_port(), ctx_size: 512, threads: Some(1), gpu_layers: 0, embedding: None,
        };
        let started = Instant::now();
        let err = LlamaServer::spawn(&cfg).await.err().expect("должна быть ошибка");
        assert!(started.elapsed() < Duration::from_secs(30));
        assert!(err.to_string().contains("llama-server"), "{err}");
    }

    #[test]
    fn free_port_is_nonzero() {
        assert_ne!(free_port(), 0);
    }
}
