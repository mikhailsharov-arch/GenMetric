#!/usr/bin/env python3
"""
Проверка стилей: весь интерфейс должен масштабироваться одной настройкой.

Роман сказал, что шрифт и межстрочный интервал крупноваты. Чинить это
подбором значений бессмысленно: у разных людей разные экраны и зрение.
Поэтому размер задан один раз переменной --ui-scale, а всё остальное
считается от него в rem и em.

Стоит появиться хоть одному размеру в пикселях — и он перестанет
масштабироваться вместе с остальным. Эта проверка такое ловит.

Исключения: толщина рамок, радиусы скругления и тени. Они от размера текста
не зависят и в пикселях заданы намеренно.

Запуск:
    python3 scripts/check_styles.py
"""

import re
import sys
from pathlib import Path

CSS = Path(__file__).resolve().parents[1] / "src" / "styles.css"

# Свойства, где пиксели допустимы: они не связаны с размером текста.
ALLOWED = ("border", "box-shadow", "outline", "border-radius")

ok_count = 0
fail_count = 0


def check(title, condition, detail=""):
    global ok_count, fail_count
    if condition:
        ok_count += 1
        print(f"  [ок]     {title}" + (f" — {detail}" if detail else ""))
    else:
        fail_count += 1
        print(f"  [ОШИБКА] {title}" + (f" — {detail}" if detail else ""))


def _utf8_stdout() -> None:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass


def main() -> int:
    _utf8_stdout()
    text = CSS.read_text(encoding="utf-8")

    print("\n1. Единый источник размера")
    check("базовый размер задан через --ui-scale",
          "font-size: calc(15px * var(--ui-scale))" in text)
    check("переменная имеет значение по умолчанию", "--ui-scale: 1;" in text)

    print("\n2. Абсолютных размеров не осталось")
    offenders = []
    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("/*") or stripped.startswith("*") or "--ui-scale" in line:
            continue
        prop = stripped.split(":")[0].strip()
        if any(prop.startswith(a) for a in ALLOWED) or "rgba" in line:
            continue
        if re.search(r"\b\d+px\b", line):
            offenders.append(f"строка {number}: {stripped[:70]}")
    check("размеры в пикселях не используются", not offenders,
          "" if not offenders else "; ".join(offenders[:3]))

    print("\n3. Межстрочные интервалы не разъезжаются")
    heights = [float(v) for v in re.findall(r"line-height:\s*([0-9.]+)\s*;", text)]
    check("интервалы заданы явно", len(heights) >= 4, f"{len(heights)} правил")
    check("нет интервалов крупнее 1.45",
          all(h <= 1.45 for h in heights), f"максимальный {max(heights) if heights else 0}")

    print("\n4. Что это даёт на экране")
    steps = [0.85, 0.925, 1, 1.1, 1.2, 1.35]
    print("     масштаб   основной текст   поле ввода   подпись поля")
    for s in steps:
        base = 15 * s
        print(f"     {int(s * 100):>4}%      {base:>5.1f} px      "
              f"{base * 1.05:>5.1f} px    {base * 0.82:>5.1f} px")
    print("     До правки основной текст был 17 px и не менялся вовсе.")

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}\n")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
