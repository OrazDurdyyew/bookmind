//! FakeLlm — предсказуемая «модель» для тестов, без настоящего llama-server.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::llm::{normalize, ChatRequest, ChatStats, EmbedKind, Llm};

/// Размер векторов FakeLlm.
pub const FAKE_DIM: usize = 64;

/// Подделка модели: эмбеддинги — «мешок слов» через хеш, ответы — заранее заданные.
pub struct FakeLlm {
    /// Ответы для chat_stream по очереди; если пусто — "NOT_IN_BOOK".
    pub chat_replies: Mutex<VecDeque<String>>,
    /// Ответы для chat_json по очереди; если пусто — ошибка.
    pub json_replies: Mutex<VecDeque<serde_json::Value>>,
    /// Начиная с этого вызова (считая с 0) embed возвращает ошибку.
    pub fail_embed_after: Option<usize>,
    pub embed_calls: AtomicUsize,
}

impl Default for FakeLlm {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeLlm {
    pub fn new() -> Self {
        FakeLlm {
            chat_replies: Mutex::new(VecDeque::new()),
            json_replies: Mutex::new(VecDeque::new()),
            fail_embed_after: None,
            embed_calls: AtomicUsize::new(0),
        }
    }

    /// Вектор текста: каждое слово (в нижнем регистре) увеличивает свою ячейку.
    pub fn vector(text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; FAKE_DIM];
        for w in text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
            // FNV-1a — простой и стабильный хеш.
            let mut h: u64 = 0xcbf29ce484222325;
            for b in w.to_lowercase().bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            v[(h % FAKE_DIM as u64) as usize] += 1.0;
        }
        if v.iter().all(|x| *x == 0.0) {
            v[0] = 1.0;
        }
        normalize(&mut v);
        v
    }
}

impl Llm for FakeLlm {
    async fn embed(&self, _kind: EmbedKind, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let call = self.embed_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_embed_after.is_some_and(|n| call >= n) {
            anyhow::bail!("FakeLlm: сервер эмбеддингов «упал»");
        }
        Ok(texts.iter().map(|t| Self::vector(t)).collect())
    }

    async fn chat_stream(&self, _req: &ChatRequest, on_token: &mut (dyn FnMut(&str) + Send)) -> anyhow::Result<(String, ChatStats)> {
        let reply = self.chat_replies.lock().unwrap().pop_front().unwrap_or_else(|| "NOT_IN_BOOK".into());
        // Отдаём по словам (с пробелами), а одно слово — двумя половинками.
        let mut pieces: Vec<String> = reply.split_inclusive(' ').map(String::from).collect();
        if pieces.len() < 2 {
            let mid = reply.char_indices().nth(reply.chars().count() / 2).map(|(i, _)| i).unwrap_or(0);
            pieces = vec![reply[..mid].to_string(), reply[mid..].to_string()];
        }
        for p in &pieces {
            on_token(p);
        }
        Ok((reply, ChatStats { ttft_ms: 1, tokens_per_sec: 100.0, completion_tokens: pieces.len() as u32 }))
    }

    async fn chat_json(&self, _req: &ChatRequest, _schema: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
        self.json_replies.lock().unwrap().pop_front().ok_or_else(|| anyhow::anyhow!("FakeLlm: нет JSON-ответа"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ChatRequest {
        ChatRequest { messages: vec![], temperature: 0.0, max_tokens: 10 }
    }

    #[tokio::test]
    async fn embed_is_unit_length_and_deterministic() {
        let f = FakeLlm::new();
        let v = f.embed(EmbedKind::Passage, &["фотосинтез в листьях".into(), "фотосинтез в листьях".into()]).await.unwrap();
        let len: f32 = v[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-5);
        assert_eq!(v[0], v[1]);
        assert_eq!(v[0].len(), FAKE_DIM);
    }

    #[tokio::test]
    async fn chat_stream_emits_scripted_reply_in_pieces() {
        let f = FakeLlm::new();
        f.chat_replies.lock().unwrap().push_back("Фотосинтез идёт [1].".into());
        let mut pieces = Vec::new();
        let (text, _) = f.chat_stream(&req(), &mut |t| pieces.push(t.to_string())).await.unwrap();
        assert_eq!(text, "Фотосинтез идёт [1].");
        assert!(pieces.len() >= 2);
        assert_eq!(pieces.concat(), text);
    }

    #[tokio::test]
    async fn fail_embed_after_makes_later_calls_fail() {
        let f = FakeLlm { fail_embed_after: Some(1), ..FakeLlm::new() };
        assert!(f.embed(EmbedKind::Passage, &["a".into()]).await.is_ok());
        assert!(f.embed(EmbedKind::Passage, &["b".into()]).await.is_err());
    }
}
