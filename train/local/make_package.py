"""Собирает лёгкий пакет для обучения на другом компьютере (~200 МБ), тяжёлое докачает fetch.py там.

Берёт Python с уже установленными и проверенными библиотеками (--env, обычно D:/bookmind-5060/python),
кладёт их колёса в wheels/ — кроме torch, которое вместе с весами Qwen3-4B уходит в manifest.json.

    uv run --no-project python train/local/make_package.py --env D:/bookmind-5060/python --out D:/bookmind-5060-lite
"""
import argparse
import json
import re
import shutil
import subprocess
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
MODEL, REV = "Qwen/Qwen3-4B", "1cfa9a7208912126459214e8b04321603b3df60c"
TORCH_INDEX = "https://download.pytorch.org/whl/cu128"
TORCH_MIRROR = "https://mirrors.aliyun.com/pytorch-wheels/cu128"
SMALL = ["config.json", "generation_config.json", "merges.txt", "model.safetensors.index.json",
         "tokenizer.json", "tokenizer_config.json", "vocab.json", "LICENSE"]


def get(url):
    with urllib.request.urlopen(url, timeout=60) as r:
        return r.read()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--env", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    out, env_py = Path(args.out), Path(args.env) / "python.exe"
    out.mkdir(parents=True, exist_ok=True)

    # Свежий Python (без пакетов) — тот же 3.12, что в проверенном окружении.
    if not (out / "python/python.exe").exists():
        tmp = out / "python-dl"
        subprocess.run(["uv", "python", "install", "3.12", "--install-dir", str(tmp)], check=True)
        src = next(p for p in tmp.iterdir() if p.is_dir() and not p.is_symlink() and re.search(r"\d+\.\d+\.\d+", p.name))
        shutil.move(str(src), out / "python")
        shutil.rmtree(tmp, ignore_errors=True)
    # uv помечает свой Python «управляемым извне» — здесь он целиком наш, pip ставит в него сам.
    for marker in (out / "python").rglob("EXTERNALLY-MANAGED"):
        marker.unlink()

    freeze = subprocess.run(["uv", "pip", "freeze", "--python", str(env_py)], check=True,
                            capture_output=True, text=True).stdout.split()
    torch_ver = next(l.split("==")[1] for l in freeze if l.startswith("torch=="))
    pins = [l for l in freeze if "==" in l and not l.startswith("torch==")]
    (out / "wheels").mkdir(exist_ok=True)
    subprocess.run(["uv", "run", "--no-project", "--with", "pip", "python", "-m", "pip", "download", "-q",
                    "--no-deps", "--only-binary", ":all:", "--platform", "win_amd64", "--python-version", "3.12",
                    "-d", str(out / "wheels"), *pins], check=True)

    whl = f"torch-{torch_ver}-cp312-cp312-win_amd64.whl"
    page = get(f"{TORCH_INDEX}/torch/").decode()
    sha = re.search(re.escape(whl.replace("+", "%2B")) + r"#sha256=([0-9a-f]+)", page).group(1)
    size = int(urllib.request.urlopen(urllib.request.Request(f"{TORCH_MIRROR}/{whl}", method="HEAD"), timeout=60)
               .headers["Content-Length"])
    files = [{"path": f"dl/{whl}", "size": size, "sha256": sha,
              "urls": [f"{TORCH_INDEX}/{whl.replace('+', '%2B')}", f"{TORCH_MIRROR}/{whl}"]}]

    tree = json.loads(get(f"https://huggingface.co/api/models/{MODEL}/tree/{REV}"))
    (out / "base").mkdir(exist_ok=True)
    for f in tree:
        name = f["path"]
        if name.endswith(".safetensors"):
            files.append({"path": f"base/{name}", "size": f["size"], "sha256": f["lfs"]["oid"], "urls": [
                f"https://huggingface.co/{MODEL}/resolve/{REV}/{name}",
                f"https://hf-mirror.com/{MODEL}/resolve/{REV}/{name}",
                f"https://modelscope.cn/models/{MODEL}/resolve/master/{name}",
            ]})
        elif name in SMALL and not (out / "base" / name).exists():
            (out / "base" / name).write_bytes(get(f"https://huggingface.co/{MODEL}/resolve/{REV}/{name}"))
    (out / "manifest.json").write_text(json.dumps({"torch": f"dl/{whl}", "files": files}, indent=1), encoding="utf-8")

    (out / "data").mkdir(exist_ok=True)
    for n in ["train.jsonl", "val.jsonl"]:
        shutil.copy(ROOT / "train/out" / n, out / "data" / n)
    for n in ["train_5060.py", "make_gguf.py", "fetch.py", "START.bat", "CHECK.bat", "ПРОЧИТАЙ.txt"]:
        shutil.copy(HERE / n, out / n)
    for d, src in [("convert", Path(args.env).parent / "convert"), ("llama", Path(args.env).parent / "llama")]:
        if not (out / d).exists():
            shutil.copytree(src, out / d)

    total = sum(p.stat().st_size for p in out.rglob("*") if p.is_file())
    heavy = sum(f["size"] for f in files)
    print(f"пакет {out}: {total / 1e6:.0f} МБ; там докачается {heavy / 1e9:.1f} ГБ")


if __name__ == "__main__":
    main()
