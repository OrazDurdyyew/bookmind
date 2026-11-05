"""Пишет manifest.json и requirements.txt — что fetch.py скачает на компьютере для обучения.

Версии берутся из проверенного окружения (--env, обычно D:/bookmind-5060/python): те же
библиотеки (с SHA-256 с PyPI), тот же torch, та же ревизия Qwen3-4B, та же сборка llama.cpp.
Запускать заново только при смене версий; результат коммитится в git.

    uv run --no-project python train/local/make_manifest.py --env D:/bookmind-5060/python
"""
import argparse
import hashlib
import json
import re
import subprocess
import tempfile
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
MODEL, REV = "Qwen/Qwen3-4B", "1cfa9a7208912126459214e8b04321603b3df60c"
TORCH_INDEX = "https://download.pytorch.org/whl/cu128"
TORCH_MIRROR = "https://mirrors.aliyun.com/pytorch-wheels/cu128"
LLAMA = "b11925"
SMALL = ["config.json", "generation_config.json", "merges.txt", "model.safetensors.index.json",
         "tokenizer.json", "tokenizer_config.json", "vocab.json", "LICENSE"]


def get(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "bookmind"}), timeout=120) as r:
        return r.read()


def model_urls(name):
    return [f"https://huggingface.co/{MODEL}/resolve/{REV}/{name}",
            f"https://hf-mirror.com/{MODEL}/resolve/{REV}/{name}",
            f"https://modelscope.cn/models/{MODEL}/resolve/master/{name}"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--env", required=True)
    args = ap.parse_args()

    freeze = subprocess.run(["uv", "pip", "freeze", "--python", str(Path(args.env) / "python.exe")],
                            check=True, capture_output=True, text=True).stdout.split()
    torch_ver = next(l.split("==")[1] for l in freeze if l.startswith("torch=="))
    pins = [l for l in freeze if "==" in l and not l.startswith("torch==")]

    # Колёса для Windows / Python 3.12 — ровно те файлы, что выбрал бы pip, с их SHA-256.
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(["uv", "run", "--no-project", "--with", "pip", "python", "-m", "pip", "download", "-q",
                        "--no-deps", "--only-binary", ":all:", "--platform", "win_amd64", "--python-version", "3.12",
                        "-d", tmp, *pins], check=True)
        reqs = []
        for whl in sorted(Path(tmp).glob("*.whl")):
            name, ver = whl.name.split("-")[:2]
            reqs.append(f"{name}=={ver} --hash=sha256:{hashlib.sha256(whl.read_bytes()).hexdigest()}")
    (HERE / "requirements.txt").write_text("\n".join(reqs) + "\n", encoding="utf-8")

    whl = f"torch-{torch_ver}-cp312-cp312-win_amd64.whl"
    page = get(f"{TORCH_INDEX}/torch/").decode()
    sha = re.search(re.escape(whl.replace("+", "%2B")) + r"#sha256=([0-9a-f]+)", page).group(1)
    size = int(urllib.request.urlopen(urllib.request.Request(f"{TORCH_MIRROR}/{whl}", method="HEAD"), timeout=60)
               .headers["Content-Length"])
    files = [{"path": f"dl/{whl}", "size": size, "sha256": sha,
              "urls": [f"{TORCH_INDEX}/{whl.replace('+', '%2B')}", f"{TORCH_MIRROR}/{whl}"]}]

    for f in json.loads(get(f"https://huggingface.co/api/models/{MODEL}/tree/{REV}")):
        name = f["path"]
        if name.endswith(".safetensors"):
            files.append({"path": f"base/{name}", "size": f["size"], "sha256": f["lfs"]["oid"], "urls": model_urls(name)})
        elif name in SMALL:
            data = get(model_urls(name)[0])
            files.append({"path": f"base/{name}", "size": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                          "urls": model_urls(name)})

    name = f"llama-{LLAMA}-bin-win-cpu-x64.zip"
    data = get(f"https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA}/{name}")
    files.append({"path": f"dl/{name}", "size": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                  "urls": [f"https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA}/{name}"],
                  "unpack": "llama"})

    (HERE / "manifest.json").write_text(json.dumps({"torch": f"dl/{whl}", "files": files}, indent=1), encoding="utf-8")
    print(f"{len(reqs)} библиотек, {len(files)} файлов, {sum(f['size'] for f in files) / 1e9:.1f} ГБ")


if __name__ == "__main__":
    main()
