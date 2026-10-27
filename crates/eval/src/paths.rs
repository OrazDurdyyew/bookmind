//! Пути проекта: корень, папка результатов, бинарник llama-server.

use std::path::{Path, PathBuf};

/// Корень проекта — папка, где лежит eval/models.toml (ищем вверх от текущей).
pub fn root() -> anyhow::Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("eval/models.toml").is_file() {
            return Ok(dir);
        }
        if !dir.pop() {
            anyhow::bail!("не найден eval/models.toml — запускайте из папки проекта");
        }
    }
}

pub fn out_dir(root: &Path) -> PathBuf {
    root.join("eval/out")
}

/// Папка прогона: `<кандидат>__<эмбеддер>__<режим>`, режим — gpu или cpu.
pub fn run_dir(root: &Path, candidate: &str, embedder: &str, mode: &str) -> PathBuf {
    out_dir(root).join(format!("{candidate}__{embedder}__{mode}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_dir_names_candidate_embedder_and_mode() {
        let d = run_dir(Path::new("/p"), "qwen3-4b", "e5-small", "cpu");
        assert_eq!(d, PathBuf::from("/p/eval/out/qwen3-4b__e5-small__cpu"));
    }
}

/// llama-server: для видеокарты — сборка CUDA или Vulkan, если она скачана.
pub fn llama_binary(root: &Path, gpu: bool) -> PathBuf {
    let name = format!("llama-server{}", std::env::consts::EXE_SUFFIX);
    if gpu {
        for variant in ["ubuntu-cuda-12.8-x64", "ubuntu-vulkan-x64"] {
            let p = root.join("tools/llama").join(variant).join(&name);
            if p.is_file() {
                return p;
            }
        }
    }
    root.join("tools/llama").join(name)
}

pub fn model_path(root: &Path, file: &str) -> PathBuf {
    root.join("models").join(file)
}
