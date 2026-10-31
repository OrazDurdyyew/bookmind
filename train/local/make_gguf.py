"""Собирает out/bookmind-tm-Q4_K_M.gguf из базовой модели и адаптера out/lora (после train_5060.py).

Адаптер вливается в Qwen3-4B, полная модель пишется в GGUF (bf16) конвертером llama.cpp
(convert/), затем сжимается до Q4_K_M программой llama/llama-quantize.exe. Промежуточные
файлы (~16 ГБ) удаляются.

    python\\python.exe make_gguf.py
"""
import argparse
import shutil
import subprocess
import sys
from pathlib import Path

import torch
from peft import PeftModel
from transformers import AutoModelForCausalLM, AutoTokenizer

HERE = Path(__file__).resolve().parent


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default=str(HERE / "base"))
    ap.add_argument("--out", default=str(HERE / "out"))
    args = ap.parse_args()
    BASE, OUT = Path(args.base), Path(args.out)
    merged, bf16 = OUT / "merged", OUT / "bookmind-tm-bf16.gguf"
    final = OUT / "bookmind-tm-Q4_K_M.gguf"
    assert (OUT / "lora" / "adapter_config.json").exists(), "нет out/lora — сначала обучение (train_5060.py)"

    print("вливаю адаптер в модель…", flush=True)
    model = AutoModelForCausalLM.from_pretrained(BASE, dtype=torch.bfloat16)
    model = PeftModel.from_pretrained(model, OUT / "lora").merge_and_unload()
    model.save_pretrained(merged)
    AutoTokenizer.from_pretrained(BASE).save_pretrained(merged)
    del model

    print("пишу GGUF…", flush=True)
    subprocess.run([sys.executable, str(HERE / "convert/convert_hf_to_gguf.py"), str(merged),
                    "--outfile", str(bf16), "--outtype", "bf16"], check=True)
    shutil.rmtree(merged)

    print("сжимаю до Q4_K_M…", flush=True)
    subprocess.run([str(HERE / "llama/llama-quantize.exe"), str(bf16), str(final), "Q4_K_M"], check=True)
    bf16.unlink()
    print(f"готово: {final} ({final.stat().st_size / 1e9:.2f} ГБ)", flush=True)


if __name__ == "__main__":
    main()
