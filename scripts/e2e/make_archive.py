#!/usr/bin/env python3
"""
Синтетический архив подсказок для сквозной проверки.

Файла Романа в конвейере нет и не будет. Архив собирается из тех же
синтетических строк, что в db/test_archive.py, теми же функциями
db/tools/build_archive.py — так проверяется настоящий формат, а не подделка.

    python scripts/e2e/make_archive.py путь/к/архив.sqlite
"""

import sqlite3
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT / "db"))
sys.path.insert(0, str(ROOT / "db" / "tools"))


def row(**cells):
    r = [None] * 55
    r[0] = cells.pop("no", 1)
    for col, v in cells.items():
        r[int(col[1:]) - 1] = v
    return r


def main() -> int:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass
    import build_seed
    import build_archive as ba

    out = Path(sys.argv[1])
    with tempfile.TemporaryDirectory() as tmp:
        seed = Path(tmp) / "seed.sqlite"
        build_seed.build(seed)
        dictionary = sqlite3.connect(seed)
        births = [
            row(no=1, c13="Чертеж Малый", c14="крестьянин", c15="Никита Алексеев",
                c19="Чертеж Малый", c20="законная жена его", c21="Евлампия Васильева",
                c31="Александр Арсеньев", c32="Чертеж Малый", c33="крестьянский сын",
                c47="Александр Рождественский", c48="священник"),
            row(no=2, c13="Чертеж Малый", c14="крестьянин", c15="Никита Алексеев",
                c19="Чертеж Малый", c20="законная жена его", c21="Евлампия Васильева",
                c31="Дария Трофимова", c32="Бухарино", c33="крестьянская дочь девица",
                c47="Александр Рождественский", c48="священник"),
        ]
        h = ba.Harvest()
        ba.harvest_rows("1", births, dictionary, h)
        dictionary.close()
    stats = ba.write_archive(h, out, "сквозная-проверка.xlsm")
    print(f"архив: {out} {stats}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
