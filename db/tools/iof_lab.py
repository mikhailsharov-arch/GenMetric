#!/usr/bin/env python3
"""Проверка гипотезы: подсказывать ИОФ пословно, а не строкой целиком."""
import csv, json, re, unicodedata
from collections import Counter
from pathlib import Path
SEED = Path("GenMetric/db/seed"); TOP = 3

def norm(v):
    if not v: return ""
    return re.sub(r"\s+"," ",unicodedata.normalize("NFC",str(v)).strip().lower().replace("ё","е"))

names = list(csv.DictReader((SEED/"name_dict.csv").open(encoding="utf-8")))
NAME_POOL = sorted({r["name"] for r in names} | {r["variant"] for r in names if r["variant"]})
PATR_POOL = sorted({f for r in names for f in (r["patr_old_m"],r["patr_old_f"],r["patr_m"],r["patr_f"]) if f})

def strokes(value, pool, seen, by_freq=True):
    t = norm(value)
    for k in range(1, len(t)+1):
        c = [x for x in pool if norm(x).startswith(t[:k])]
        c.sort(key=lambda x: (-seen[norm(x)], norm(x)) if by_freq else (norm(x),))
        if t in [norm(x) for x in c[:TOP]]: return k
    return len(t)

persons = json.load(open("persons.json", encoding="utf-8"))
seen_n, seen_p, seen_f = Counter(), Counter(), Counter()
fam_pool = []
tot = {"chars":0,"tok":0,"whole":0}

for p in persons:
    toks = [t for t in re.split(r"\s+", (p["iof"] or "").strip()) if t]
    if not toks: continue
    tot["chars"] += len(norm(p["iof"]))
    # пословно: имя из словаря имён, отчество из форм отчеств, фамилия из ранее встреченных
    cost = strokes(toks[0], NAME_POOL, seen_n); seen_n[norm(toks[0])] += 1
    if len(toks) > 1:
        cost += 1 + strokes(toks[1], PATR_POOL, seen_p); seen_p[norm(toks[1])] += 1
    if len(toks) > 2:
        rest = " ".join(toks[2:])
        cost += 1 + strokes(rest, fam_pool, seen_f); seen_f[norm(rest)] += 1
        if norm(rest) not in {norm(x) for x in fam_pool}: fam_pool.append(rest)
    tot["tok"] += cost

print(f"ИОФ, {len(persons)} персон, {tot['chars']} символов если печатать целиком\n")
print(f"  подсказка строкой целиком : 48603 нажатий  экономия 18.4%   (замер из rank_lab)")
print(f"  подсказка по словам       : {tot['tok']:>5} нажатий  экономия {(tot['chars']-tot['tok'])/tot['chars']*100:4.1f}%")
print(f"\n  выигрыш пословного разбора: {48603-tot['tok']} нажатий")
