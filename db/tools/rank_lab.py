#!/usr/bin/env python3
"""
Измерение качества автоподстановки на настоящей работе.

Проигрываем 3664 персоны прихода Борисоглебское в том порядке, в каком их
набирал Роман, и для каждого поля считаем: сколько букв нужно напечатать,
чтобы верное значение оказалось в первой тройке подсказок.

Сравниваем два порядка выдачи:
  «по алфавиту» — как в нынешнем Excel-Индексаторе;
  «по частоте»  — как просит заказчик и как сделано у нас.

Разница между этими двумя числами и есть цена главной претензии из опросника.
"""

import csv
import json
import re
import unicodedata
from collections import Counter
from pathlib import Path

SEED = Path("GenMetric/db/seed")
TOP = 3


def norm(v) -> str:
    if not v:
        return ""
    s = unicodedata.normalize("NFC", str(v)).strip().lower().replace("ё", "е")
    return re.sub(r"\s+", " ", s)


def load_lookup(kind: str) -> list:
    return [r["value"] for r in csv.DictReader((SEED / "lookup.csv").open(encoding="utf-8"))
            if r["kind"] == kind]


def keystrokes(value, candidates_at, by_freq: bool, seen: Counter) -> int:
    """Сколько букв нужно набрать, чтобы значение попало в первую тройку.

    Возвращает len(value), если подсказка так и не помогла — то есть слово
    пришлось напечатать целиком.
    """
    target = norm(value)
    for k in range(1, len(target) + 1):
        prefix = target[:k]
        cands = [c for c in candidates_at if norm(c).startswith(prefix)]
        if by_freq:
            cands.sort(key=lambda c: (-seen[norm(c)], norm(c)))
        else:
            cands.sort(key=norm)
        if target in [norm(c) for c in cands[:TOP]]:
            return k
    return len(target)


def simulate(field: str, values: list, dictionary: list) -> dict:
    """Проигрывает поле по всем записям подряд."""
    seen = Counter()
    known = list(dictionary)
    known_norm = {norm(c) for c in known}
    stats = {"freq": 0, "abc": 0, "chars": 0, "n": 0,
             "freq_first_letter": 0, "abc_first_letter": 0}

    for v in values:
        if not v:
            continue
        stats["n"] += 1
        stats["chars"] += len(norm(v))
        k_freq = keystrokes(v, known, True, seen)
        k_abc = keystrokes(v, known, False, seen)
        stats["freq"] += k_freq
        stats["abc"] += k_abc
        stats["freq_first_letter"] += (k_freq <= 1)
        stats["abc_first_letter"] += (k_abc <= 1)
        # значение набрано — оно попадает в справочник и в статистику частот
        seen[norm(v)] += 1
        if norm(v) not in known_norm:
            known.append(v)
            known_norm.add(norm(v))
    return stats


def report(title: str, s: dict) -> None:
    if not s["n"]:
        return
    n, chars = s["n"], s["chars"]
    saved_freq = (chars - s["freq"]) / chars * 100
    saved_abc = (chars - s["abc"]) / chars * 100
    print(f"\n  {title}  ({n} заполнений, {chars} символов если печатать целиком)")
    print(f"    по частоте:   {s['freq']:>6} нажатий   экономия {saved_freq:4.1f}%   "
          f"с первой буквы {s['freq_first_letter'] / n * 100:4.1f}%")
    print(f"    по алфавиту:  {s['abc']:>6} нажатий   экономия {saved_abc:4.1f}%   "
          f"с первой буквы {s['abc_first_letter'] / n * 100:4.1f}%")
    diff = s["abc"] - s["freq"]
    print(f"    выигрыш от сортировки по частоте: {diff} нажатий "
          f"({diff / max(s['abc'], 1) * 100:.1f}% от алфавитного варианта)")


def main() -> None:
    persons = json.load(open("persons.json", encoding="utf-8"))
    print(f"Проиграно {len(persons)} персон в порядке набора\n")
    print("Считаем, сколько букв нужно, чтобы верное значение попало в первую тройку.")
    print("Начальное состояние: справочники как в поставке, история пуста.")

    total = {"freq": 0, "abc": 0, "chars": 0}

    for field, kind, title in [
        ("rank", "rank_m", "Звание"),
        ("place", None, "Населённый пункт"),
        ("iof", None, "ИОФ персоны"),
    ]:
        values = [p[field] for p in persons]
        dictionary = load_lookup(kind) if kind else []
        s = simulate(field, values, dictionary)
        report(title, s)
        for k in ("freq", "abc", "chars"):
            total[k] += s[k]

    print("\n  ---")
    print(f"  ИТОГО по трём полям: печатать целиком {total['chars']} символов")
    print(f"    по частоте  {total['freq']:>6}  — экономия {(total['chars'] - total['freq']) / total['chars'] * 100:.1f}%")
    print(f"    по алфавиту {total['abc']:>6}  — экономия {(total['chars'] - total['abc']) / total['chars'] * 100:.1f}%")


if __name__ == "__main__":
    main()
