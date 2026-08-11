#!/usr/bin/env python3
"""
Лаборатория разбора ИОФ.

Проверяем на настоящих данных, можно ли надёжно разобрать строку вида
«Никита Алексеев» или «Ирина Федорова Борисова» на имя, отчество и фамилию,
опираясь только на словарь имён. От этого зависит, можно ли объединить три
поля ввода в одно, как просит заказчик (пункт 5 отчёта о тестировании).

Данные: 3664 персоны из рабочего файла по Борисоглебскому приходу,
где имя, отчество и фамилия уже разнесены по колонкам — это эталон.

Важно: разбиение строки и осовременивание написания — разные задачи, и
меряются они отдельно. Разбиение обязано быть безошибочным, потому что от
него зависит структура данных. Осовременивание — это предложение
пользователю, которое он может изменить.
"""

import csv
import json
import re
import unicodedata
from collections import Counter
from pathlib import Path

SEED = Path("GenMetric/db/seed")


def norm(v) -> str:
    if not v:
        return ""
    s = unicodedata.normalize("NFC", str(v)).strip().lower().replace("ё", "е")
    return re.sub(r"\s+", " ", s)


# --- словарь ----------------------------------------------------------------
names = list(csv.DictReader((SEED / "name_dict.csv").open(encoding="utf-8")))

NAME_BY_NORM = {}   # написание имени -> строка словаря
PATR_BY_NORM = {}   # любая форма отчества -> (имя отца, современная форма, пол)

# Точное совпадение с заголовочным именем имеет приоритет над совпадением
# с вариантом написания. Иначе «Никита», который есть и сам по себе, и как
# вариант «Аникиты», превращался бы в «Аникиту».
for row in names:
    NAME_BY_NORM.setdefault(norm(row["name"]), row)
for row in names:
    if row["variant"]:
        NAME_BY_NORM.setdefault(norm(row["variant"]), row)
    for old_form, modern, sex in (
        (row["patr_old_m"], row["patr_m"], "М"),
        (row["patr_old_f"], row["patr_f"], "Ж"),
        (row["patr_m"], row["patr_m"], "М"),
        (row["patr_f"], row["patr_f"], "Ж"),
    ):
        if old_form:
            PATR_BY_NORM.setdefault(norm(old_form), (row["name"], modern, sex))


def parse(text: str) -> dict:
    """Разбирает строку на фамилию, имя и отчество.

    Порядок в метрических книгах: имя, отчество, фамилия. Отчество опознаётся
    по словарю — и старая форма «Алексеев», и современная «Алексеевич», —
    поэтому второе слово, которого нет среди отчеств, считается фамилией.

    Написание НЕ подменяется: в полях остаётся то, что написано в книге.
    Современный вариант выдаётся отдельно, как предложение.
    """
    tokens = [t for t in re.split(r"\s+", (text or "").strip()) if t]
    out = {"first_name": None, "first_name_modern": None,
           "patronymic": None, "patronymic_modern": None,
           "surname": None, "gender": None, "father_name": None,
           "known_name": False}
    if not tokens:
        return out

    out["first_name"] = tokens[0]
    head = NAME_BY_NORM.get(norm(tokens[0]))
    if head:
        out["known_name"] = True
        out["gender"] = head["gender"] or None
        # современная форма: имя-основа, если оно задано, иначе само имя
        out["first_name_modern"] = head["base_name"] or head["name"]

    rest = tokens[1:]
    if rest:
        patr = PATR_BY_NORM.get(norm(rest[0]))
        if patr:
            father, modern, sex = patr
            out["patronymic"] = rest[0]
            out["patronymic_modern"] = modern
            out["father_name"] = father
            out["gender"] = out["gender"] or sex
            rest = rest[1:]
    if rest:
        out["surname"] = " ".join(rest)
    return out


def base_of(value: str) -> str:
    """Приводит имя к словарной основе — чтобы «Наталия» и «Наталья»
    считались одним именем при сравнении с эталоном."""
    row = NAME_BY_NORM.get(norm(value))
    if not row:
        return norm(value)
    return norm(row["base_name"] or row["name"])


def main() -> None:
    persons = json.load(open("persons.json", encoding="utf-8"))
    total = len(persons)
    c = Counter()
    misses = {"split_name": [], "split_patr": [], "split_fam": [], "modern": []}

    for p in persons:
        got = parse(p["iof"])
        src_tokens = [t for t in re.split(r"\s+", (p["iof"] or "").strip()) if t]

        # --- 1. Разбиение: тот ли токен попал в то поле ---------------------
        # Имя: считаем верным, если выбранное слово и эталон — одно и то же имя
        # с точностью до словарного варианта написания.
        ok_split_name = base_of(got["first_name"]) == base_of(p["name"])

        # Отчество: эталон может быть заполнен не из строки, а выведен из имени
        # отца в той же записи — такие случаи разбором не проверить.
        patr_in_string = len(src_tokens) > 1
        if not patr_in_string:
            c["patr_not_in_string"] += 1
            ok_split_patr = None
        else:
            ok_split_patr = norm(got["patronymic_modern"] or got["patronymic"]) == norm(p["patr"])
            c["patr_checked"] += 1
            c["patr_ok"] += ok_split_patr

        ok_split_fam = norm(got["surname"]) == norm(p["fam"])

        c["name_ok"] += ok_split_name
        c["fam_ok"] += ok_split_fam
        c["gender_ok"] += (got["gender"] or "") == (p["sex"] or "")
        if p["fam"]:
            c["with_fam"] += 1
            c["with_fam_ok"] += ok_split_fam
        c["split_all_ok"] += ok_split_name and ok_split_fam and (ok_split_patr is not False)

        # --- 2. Осовременивание: совпало ли предложение с выбором человека --
        if got["first_name_modern"]:
            c["modern_checked"] += 1
            same = norm(got["first_name_modern"]) == norm(p["name"])
            c["modern_ok"] += same
            if not same and len(misses["modern"]) < 10:
                misses["modern"].append((p["iof"], got["first_name_modern"], p["name"]))

        if not ok_split_name and len(misses["split_name"]) < 10:
            misses["split_name"].append((p["iof"], got["first_name"], p["name"]))
        if ok_split_patr is False and len(misses["split_patr"]) < 10:
            misses["split_patr"].append((p["iof"], got["patronymic_modern"], p["patr"]))
        if not ok_split_fam and len(misses["split_fam"]) < 10:
            misses["split_fam"].append((p["iof"], got["surname"], p["fam"]))

    pct = lambda n, d=total: f"{n / d * 100:5.1f}%"
    print(f"Разбор ИОФ проверен на {total} настоящих персонах прихода Борисоглебское\n")

    print("1. РАЗБИЕНИЕ СТРОКИ — то, от чего зависит структура данных")
    print(f"   имя выделено верно          {c['name_ok']:>5} из {total}   {pct(c['name_ok'])}")
    print(f"   отчество выделено верно     {c['patr_ok']:>5} из {c['patr_checked']}   {pct(c['patr_ok'], c['patr_checked'])}")
    print(f"   фамилия выделена верно      {c['fam_ok']:>5} из {total}   {pct(c['fam_ok'])}")
    print(f"     в том числе где она есть  {c['with_fam_ok']:>5} из {c['with_fam']}   {pct(c['with_fam_ok'], c['with_fam'])}")
    print(f"   пол определён верно         {c['gender_ok']:>5} из {total}   {pct(c['gender_ok'])}")
    print(f"   вся строка разбита верно    {c['split_all_ok']:>5} из {total}   {pct(c['split_all_ok'])}")
    print(f"   (у {c['patr_not_in_string']} персон отчества в строке нет — оно выведено из имени отца,")
    print("    разбором это не проверяется)")

    print("\n2. ОСОВРЕМЕНИВАНИЕ — предложение, которое человек может изменить")
    print(f"   совпало с выбором человека  {c['modern_ok']:>5} из {c['modern_checked']}   {pct(c['modern_ok'], c['modern_checked'])}")

    for key, title in [("split_name", "РАЗБИЕНИЕ, имя"), ("split_patr", "РАЗБИЕНИЕ, отчество"),
                       ("split_fam", "РАЗБИЕНИЕ, фамилия"), ("modern", "ОСОВРЕМЕНИВАНИЕ")]:
        if misses[key]:
            print(f"\n   Расхождения — {title}:")
            for src, got, want in misses[key][:8]:
                print(f"     {src!r:36} получилось {got!r:20} у Романа {want!r}")


if __name__ == "__main__":
    main()
