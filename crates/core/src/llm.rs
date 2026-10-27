//! Доступ к языковой модели: трейт `Llm` и клиент llama-server по HTTP.

use std::future::Future;

use serde::Serialize;

/// Роль сообщения в чате.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub temperature: f32,
    pub max_tokens: u32,
}

/// Скорость ответа: время до первого текста и токены в секунду.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChatStats {
    pub ttft_ms: u64,
    pub tokens_per_sec: f32,
    pub completion_tokens: u32,
}

/// Что встраиваем: вопрос пользователя или фрагмент книги.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedKind {
    Query,
    Passage,
}

/// Всё, что «мотору» нужно от модели. Настоящая реализация — `LlamaHttp`,
/// в тестах — `FakeLlm`.
pub trait Llm {
    /// Эмбеддинги текстов, нормированные по длине (L2).
    fn embed(&self, kind: EmbedKind, texts: &[String]) -> impl Future<Output = anyhow::Result<Vec<Vec<f32>>>> + Send;
    /// Ответ по кусочкам: `on_token` вызывается для каждого нового кусочка текста.
    fn chat_stream(
        &self,
        req: &ChatRequest,
        on_token: &mut (dyn FnMut(&str) + Send),
    ) -> impl Future<Output = anyhow::Result<(String, ChatStats)>> + Send;
    /// Ответ строго в формате JSON по схеме.
    fn chat_json(&self, req: &ChatRequest, schema: &serde_json::Value) -> impl Future<Output = anyhow::Result<serde_json::Value>> + Send;
}

/// Нормирует вектор до единичной длины (нулевой вектор не меняется).
pub fn normalize(v: &mut [f32]) {
    let len = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if len > 0.0 {
        v.iter_mut().for_each(|x| *x /= len);
    }
}

/// Одно событие из потока SSE llama-server.
#[derive(Debug, PartialEq)]
pub enum SseEvent {
    /// Новый кусочек текста и, если есть, итоговая статистика.
    Chunk { content: String, tokens_per_sec: Option<f32>, completion_tokens: Option<u32> },
    Done,
}

/// Разбирает одну строку SSE (`data: {...}`). Пустые и служебные строки → None.
pub fn parse_sse_line(line: &str) -> Option<SseEvent> {
    let data = line.trim().strip_prefix("data:")?.trim();
    if data == "[DONE]" {
        return Some(SseEvent::Done);
    }
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let content = v["choices"][0]["delta"]["content"].as_str().unwrap_or("").to_string();
    let tokens_per_sec = v["timings"]["predicted_per_second"].as_f64().map(|x| x as f32);
    let completion_tokens = v["timings"]["predicted_n"].as_u64().map(|x| x as u32);
    Some(SseEvent::Chunk { content, tokens_per_sec, completion_tokens })
}

/// Модели эмбеддингов (e5, bge) принимают не больше 512 токенов.
/// 300 слов — с запасом; длиннее текст обрезаем, иначе сервер вернёт ошибку.
pub const MAX_EMBED_WORDS: usize = 300;

/// И не больше стольких символов: в тексте с формулами и номерами
/// («=221.=—CO₂») на одно слово приходится много токенов.
pub const MAX_EMBED_CHARS: usize = 1500;

/// Текст для эмбеддинга: префикс модели + не больше MAX_EMBED_WORDS слов
/// и MAX_EMBED_CHARS символов.
pub fn embed_input(prefix: &str, text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(MAX_EMBED_WORDS).collect();
    format!("{prefix}{}", words.join(" ")).chars().take(MAX_EMBED_CHARS).collect()
}

/// Первая половина слов текста (минимум одно слово).
pub fn halve_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    words[..(words.len() / 2).max(1).min(words.len())].join(" ")
}

/// Префиксы для эмбеддингов: у e5 «query: »/«passage: », у bge-m3 пусто.
#[derive(Clone, Debug, Default)]
pub struct EmbedPrefixes {
    pub query: String,
    pub passage: String,
}

/// Клиент двух процессов llama-server: чат и эмбеддинги.
pub struct LlamaHttp {
    chat_base: String,
    embed_base: String,
    prefixes: EmbedPrefixes,
    http: reqwest::Client,
}

impl LlamaHttp {
    pub fn new(chat_base: &str, embed_base: &str, prefixes: EmbedPrefixes) -> Self {
        LlamaHttp {
            chat_base: chat_base.trim_end_matches('/').to_string(),
            embed_base: embed_base.trim_end_matches('/').to_string(),
            prefixes,
            http: reqwest::Client::new(),
        }
    }

    fn chat_body(req: &ChatRequest, stream: bool) -> serde_json::Value {
        serde_json::json!({
            "messages": req.messages,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
            "stream": stream,
            // Выключаем «размышления» у моделей Qwen3: нам нужен сразу ответ.
            "chat_template_kwargs": { "enable_thinking": false },
        })
    }
}

impl LlamaHttp {
    /// Один запрос /v1/embeddings. Текст ошибки сервера попадает в сообщение.
    async fn post_embeddings(&self, input: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let resp = self.http.post(format!("{}/v1/embeddings", self.embed_base)).json(&serde_json::json!({ "input": input })).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("эмбеддинги: {status}: {}", resp.text().await.unwrap_or_default());
        }
        let resp: serde_json::Value = resp.json().await?;
        let mut items: Vec<(u64, Vec<f32>)> = resp["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("нет data в ответе эмбеддингов"))?
            .iter()
            .map(|d| {
                let v = d["embedding"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default();
                (d["index"].as_u64().unwrap_or(0), v)
            })
            .collect();
        items.sort_by_key(|(i, _)| *i);
        if items.len() != input.len() {
            anyhow::bail!("ожидалось {} эмбеддингов, получено {}", input.len(), items.len());
        }
        Ok(items.into_iter().map(|(_, v)| v).collect())
    }
}

impl Llm for LlamaHttp {
    async fn embed(&self, kind: EmbedKind, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let prefix = match kind {
            EmbedKind::Query => &self.prefixes.query,
            EmbedKind::Passage => &self.prefixes.passage,
        };
        let input: Vec<String> = texts.iter().map(|t| embed_input(prefix, t)).collect();
        let mut out = match self.post_embeddings(&input).await {
            Ok(v) => v,
            // Какой-то текст всё же длиннее 512 токенов: по одному, укорачивая вдвое.
            Err(e) if e.to_string().contains("too large") => {
                let mut all = Vec::with_capacity(input.len());
                for text in &input {
                    let mut t = text.clone();
                    let mut attempt = 0;
                    loop {
                        match self.post_embeddings(std::slice::from_ref(&t)).await {
                            Ok(mut v) => {
                                all.push(v.remove(0));
                                break;
                            }
                            Err(e) if e.to_string().contains("too large") && attempt < 4 => {
                                t = halve_words(&t);
                                attempt += 1;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }
                all
            }
            Err(e) => return Err(e),
        };
        out.iter_mut().for_each(|v| normalize(v));
        Ok(out)
    }

    async fn chat_stream(&self, req: &ChatRequest, on_token: &mut (dyn FnMut(&str) + Send)) -> anyhow::Result<(String, ChatStats)> {
        use futures_util::StreamExt;

        let started = std::time::Instant::now();
        let resp = self
            .http
            .post(format!("{}/v1/chat/completions", self.chat_base))
            .json(&Self::chat_body(req, true))
            .send()
            .await?
            .error_for_status()?;
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut text = String::new();
        let mut stats = ChatStats::default();
        'outer: while let Some(bytes) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&bytes?));
            // Обрабатываем все полные строки, хвост оставляем в буфере.
            while let Some(pos) = buf.find('\n') {
                let line: String = buf.drain(..=pos).collect();
                match parse_sse_line(&line) {
                    Some(SseEvent::Done) => break 'outer,
                    Some(SseEvent::Chunk { content, tokens_per_sec, completion_tokens }) => {
                        if !content.is_empty() {
                            if text.is_empty() {
                                stats.ttft_ms = started.elapsed().as_millis().max(1) as u64;
                            }
                            text.push_str(&content);
                            on_token(&content);
                        }
                        if let Some(t) = tokens_per_sec {
                            stats.tokens_per_sec = t;
                        }
                        if let Some(n) = completion_tokens {
                            stats.completion_tokens = n;
                        }
                    }
                    None => {}
                }
            }
        }
        Ok((text, stats))
    }

    async fn chat_json(&self, req: &ChatRequest, schema: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let mut body = Self::chat_body(req, false);
        body["response_format"] = serde_json::json!({ "type": "json_schema", "json_schema": { "schema": schema } });
        let resp: serde_json::Value = self
            .http
            .post(format!("{}/v1/chat/completions", self.chat_base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("в ответе нет content"))?;
        Ok(serde_json::from_str(content)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_content_timings_and_done() {
        let line = r#"data: {"choices":[{"delta":{"content":"При"}}]}"#;
        assert_eq!(
            parse_sse_line(line),
            Some(SseEvent::Chunk { content: "При".into(), tokens_per_sec: None, completion_tokens: None })
        );
        let last = r#"data: {"choices":[{"delta":{}}],"timings":{"predicted_per_second":12.5,"predicted_n":40}}"#;
        assert_eq!(
            parse_sse_line(last),
            Some(SseEvent::Chunk { content: String::new(), tokens_per_sec: Some(12.5), completion_tokens: Some(40) })
        );
        assert_eq!(parse_sse_line("data: [DONE]"), Some(SseEvent::Done));
        assert_eq!(parse_sse_line(": ping"), None);
        assert_eq!(parse_sse_line(""), None);
    }

    #[test]
    fn embed_input_adds_prefix_and_caps_length() {
        assert_eq!(embed_input("query: ", "где лист"), "query: где лист");
        let long: String = (0..1000).map(|i| format!("w{i} ")).collect();
        let out = embed_input("passage: ", &long);
        assert_eq!(out.split_whitespace().count(), MAX_EMBED_WORDS + 1); // + слово префикса
        assert!(out.starts_with("passage: w0 "));
        // Длинные «слова» (формулы, номера) ограничены ещё и по символам.
        let dense = "=221.=—CO₂ ".repeat(400);
        assert!(embed_input("", &dense).chars().count() <= MAX_EMBED_CHARS);
    }

    #[test]
    fn halve_words_shrinks_text() {
        assert_eq!(halve_words("a b c d e"), "a b");
        assert_eq!(halve_words("one"), "one");
    }

    #[test]
    fn normalize_makes_unit_length() {
        let mut v = vec![3.0, 4.0];
        normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
    }
}
