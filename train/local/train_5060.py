"""Дообучение Qwen3-4B на туркменских примерах на своей видеокарте (RTX 5060 Ti 16 ГБ), без интернета.

То же, что train/kaggle_finetune.ipynb, но без Unsloth: обычный PyTorch + PEFT, модель в bf16
(4 ГБ параметров × 2 байта = 8 ГБ — на 16 ГБ влезает без 4-битного сжатия). Учим только ответу
модели; логиты считаем лишь на позициях ответа — иначе на 7000 токенов они одни заняли бы ~9 ГБ.

Папка рядом со скриптом: python/ (Python с библиотеками), base/ (Qwen/Qwen3-4B),
data/train.jsonl и data/val.jsonl. Результат — out/lora. Каждые 25 шагов сохраняется
out/checkpoint: если компьютер выключится, повторный запуск продолжит с него.

    python\\python.exe train_5060.py
    python\\python.exe train_5060.py --limit 16 --max-len 2048   # быстрая проверка
"""
import argparse
import json
import math
import os
import random
import sys
import time
from pathlib import Path

import torch
from peft import LoraConfig, PeftModel, get_peft_model
from transformers import AttentionInterface, AutoModelForCausalLM, AutoTokenizer, get_cosine_schedule_with_warmup

HERE = Path(__file__).resolve().parent
LOG = None


def attention(module, q, k, v, attention_mask, scaling=None, dropout=0.0, **kwargs):
    """Внимание для одного примера без заполнения. У Qwen3 ключей вдвое меньше голов, чем у
    запросов, а экономное ядро PyTorch (в сборке для Windows нет flash) такое не берёт и молча
    падает на «math», которое хранит матрицу n×n: на 7000 токенов это десятки ГБ. Поэтому ключи
    размножаются до числа голов запросов."""
    n = q.shape[1] // k.shape[1]
    if n > 1:
        k, v = k.repeat_interleave(n, dim=1), v.repeat_interleave(n, dim=1)
    causal = q.shape[2] > 1  # при генерации по одному токену маска не нужна
    o = torch.nn.functional.scaled_dot_product_attention(q, k, v, is_causal=causal, scale=scaling)
    return o.transpose(1, 2).contiguous(), None


AttentionInterface.register("bookmind", attention)


def log(*a):
    s = " ".join(str(x) for x in a)
    print(s, flush=True)
    if LOG:
        LOG.write(s + "\n")
        LOG.flush()


def encode(tok, messages, max_len):
    """Токены примера и метки: -100 везде, кроме ответа модели."""
    prompt = tok.apply_chat_template(messages[:-1], tokenize=False, add_generation_prompt=True, enable_thinking=False)
    full = tok.apply_chat_template(messages, tokenize=False, enable_thinking=False)
    assert full.startswith(prompt), "шаблон чата дал разное начало для вопроса и для полного примера"
    p = tok(prompt, add_special_tokens=False)["input_ids"]
    ids = tok(full, add_special_tokens=False)["input_ids"]
    if len(ids) > max_len:
        return None
    return ids, [-100] * len(p) + ids[len(p):]


def load(tok, path, max_len, limit):
    out, long = [], 0
    for line in path.open(encoding="utf-8"):
        e = encode(tok, json.loads(line)["messages"], max_len)
        if e is None:
            long += 1
            continue
        out.append(e)
        if limit and len(out) >= limit:
            break
    return out, long


def loss_on(model, ids, labels):
    """Средняя ошибка на токенах ответа; lm_head применяется только к ним."""
    base = model.get_base_model()
    x = torch.tensor([ids], device="cuda")
    y = torch.tensor(labels[1:], device="cuda")
    h = base.model(input_ids=x).last_hidden_state[0, :-1]
    keep = y != -100
    logits = base.lm_head(h[keep]).float()
    return torch.nn.functional.cross_entropy(logits, y[keep])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default=str(HERE / "base"))
    ap.add_argument("--data", default=str(HERE / "data"))
    ap.add_argument("--out", default=str(HERE / "out"))
    ap.add_argument("--max-len", type=int, default=7168)
    ap.add_argument("--epochs", type=int, default=1)
    ap.add_argument("--lr", type=float, default=1e-4)
    ap.add_argument("--grad-accum", type=int, default=8)
    ap.add_argument("--limit", type=int, default=0, help="взять только столько примеров (для проверки)")
    args = ap.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    global LOG
    LOG = (out / "log.txt").open("a", encoding="utf-8")
    t0 = time.time()

    assert torch.cuda.is_available(), "PyTorch не видит видеокарту: обновите драйвер NVIDIA (нужен 572 или новее)"
    log("видеокарта:", torch.cuda.get_device_name(0), f"{torch.cuda.get_device_properties(0).total_memory / 2**30:.1f} ГБ")
    torch.ones(8, 8, device="cuda", dtype=torch.bfloat16) @ torch.ones(8, 8, device="cuda", dtype=torch.bfloat16)

    tok = AutoTokenizer.from_pretrained(args.base)
    train, long = load(tok, Path(args.data) / "train.jsonl", args.max_len, args.limit)
    val, _ = load(tok, Path(args.data) / "val.jsonl", args.max_len, args.limit and 4)
    log(f"примеров: {len(train)} для обучения, {len(val)} для проверки (длиннее {args.max_len} токенов пропущено: {long})")

    model = AutoModelForCausalLM.from_pretrained(args.base, dtype=torch.bfloat16, attn_implementation="bookmind").to("cuda")
    model.config.use_cache = False
    model.gradient_checkpointing_enable(gradient_checkpointing_kwargs={"use_reentrant": False})
    model.enable_input_require_grads()
    ckpt = out / "checkpoint"
    if (ckpt / "adapter_config.json").exists():
        model = PeftModel.from_pretrained(model, ckpt, is_trainable=True)
    else:
        model = get_peft_model(model, LoraConfig(
            r=16, lora_alpha=32, lora_dropout=0.0, task_type="CAUSAL_LM",
            target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
        ))

    params = [p for p in model.parameters() if p.requires_grad]
    opt = torch.optim.AdamW(params, lr=args.lr, weight_decay=0.01)
    order = []
    for ep in range(args.epochs):
        idx = list(range(len(train)))
        random.Random(3407 + ep).shuffle(idx)
        order += idx
    total = math.ceil(len(order) / args.grad_accum)
    sched = get_cosine_schedule_with_warmup(opt, 10, total)
    step = 0
    if (ckpt / "state.pt").exists():
        st = torch.load(ckpt / "state.pt", weights_only=False)
        opt.load_state_dict(st["opt"])
        sched.load_state_dict(st["sched"])
        step = st["step"]
        log(f"продолжаю с шага {step} (из прошлого запуска)")

    model.train()
    run_t0, run_step, losses = time.time(), step, []
    while step < total:
        batch = order[step * args.grad_accum:(step + 1) * args.grad_accum]
        for i in batch:
            loss = loss_on(model, *train[i]) / len(batch)
            loss.backward()
            losses.append(loss.item() * len(batch))
        torch.nn.utils.clip_grad_norm_(params, 1.0)
        opt.step()
        sched.step()
        opt.zero_grad(set_to_none=True)
        step += 1
        if step % 5 == 0 or step == total:
            per = (time.time() - run_t0) / (step - run_step)
            log(f"шаг {step}/{total}, ошибка {sum(losses) / len(losses):.3f}, прошло {(time.time() - t0) / 3600:.1f} ч, "
                f"осталось ~{per * (total - step) / 3600:.1f} ч, память {torch.cuda.max_memory_allocated() / 2**30:.1f} ГБ")
            losses = []
        if step % 25 == 0 and step < total:
            model.save_pretrained(ckpt)
            torch.save({"opt": opt.state_dict(), "sched": sched.state_dict(), "step": step}, ckpt / "state.pt")

    model.save_pretrained(out / "lora")
    tok.save_pretrained(out / "lora")
    log("обучение закончено, адаптер:", out / "lora")

    model.eval()
    with torch.no_grad():
        v = [loss_on(model, *e).item() for e in val]
        log(f"ошибка на проверочных примерах: {sum(v) / len(v):.3f}")
        model.config.use_cache = True
        for line in list((Path(args.data) / "val.jsonl").open(encoding="utf-8"))[:3]:
            msgs = json.loads(line)["messages"]
            prompt = tok.apply_chat_template(msgs[:-1], tokenize=False, add_generation_prompt=True, enable_thinking=False)
            x = tok(prompt, return_tensors="pt", add_special_tokens=False).to("cuda")
            y = model.generate(**x, max_new_tokens=150, do_sample=False)
            log("ВОПРОС:", msgs[-2]["content"].split("Sorag:")[-1].strip())
            log("ОЖИДАЛОСЬ:", msgs[-1]["content"])
            log("МОДЕЛЬ:", tok.decode(y[0][x["input_ids"].shape[1]:], skip_special_tokens=True).strip())
            log("---")
    log(f"всего {(time.time() - t0) / 3600:.1f} ч")


if __name__ == "__main__":
    sys.exit(main())
