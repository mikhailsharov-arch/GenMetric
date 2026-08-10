#!/usr/bin/env python3
"""
Генерация иконок приложения.

Иконки рисуются кодом и кладутся в репозиторий готовыми, чтобы сборка не
зависела ни от графических редакторов, ни от внешних утилит. Если понадобится
другой рисунок — правьте этот файл и перезапустите.

Запуск (нужен Pillow):
    python3 src-tauri/icons/make_icons.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent
S = 1024                     # размер исходника, остальное масштабируется
BG = (31, 78, 121, 255)      # тот же синий, что и в интерфейсе
PAPER = (255, 255, 255, 255)
LINE = (150, 178, 205, 255)
SPINE = (222, 233, 243, 255)


def rounded_background() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, S - 1, S - 1], radius=int(S * 0.22), fill=BG)
    return img


def draw_book(img: Image.Image) -> None:
    """Раскрытая книга: две страницы, слегка приподнятые к корешку."""
    d = ImageDraw.Draw(img)
    left, right = int(S * 0.13), int(S * 0.87)
    top, bottom = int(S * 0.28), int(S * 0.75)
    mid = S // 2
    lift = int(S * 0.045)          # насколько край страницы ниже корешка
    curve = int(S * 0.025)

    d.polygon([(left, top + lift), (mid, top), (mid, bottom), (left, bottom + lift - curve)], fill=PAPER)
    d.polygon([(right, top + lift), (mid, top), (mid, bottom), (right, bottom + lift - curve)], fill=PAPER)
    d.line([(mid, top), (mid, bottom)], fill=SPINE, width=int(S * 0.012))

    # Строки записей: короткие штрихи, имитирующие рукописный текст.
    rows = 5
    step = (bottom - top - int(S * 0.09)) // rows
    for i in range(rows):
        y = top + int(S * 0.075) + i * step
        pad = int(S * 0.035)
        sag = int(lift * (1 - i / (rows * 1.6)))
        w = max(2, int(S * 0.012))
        d.line([(left + pad, y + sag), (mid - pad, y)], fill=LINE, width=w)
        end = right - pad - (int(S * 0.10) if i == rows - 1 else 0)
        d.line([(mid + pad, y), (end, y + sag)], fill=LINE, width=w)


def main() -> None:
    base = rounded_background()
    draw_book(base)

    base.resize((512, 512), Image.LANCZOS).save(OUT / "icon.png")
    for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png")]:
        base.resize((size, size), Image.LANCZOS).save(OUT / name)

    base.resize((256, 256), Image.LANCZOS).save(
        OUT / "icon.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    base.save(OUT / "icon.icns")

    for f in sorted(OUT.glob("*")):
        if f.suffix in {".png", ".ico", ".icns"}:
            print(f"  {f.name:<18} {f.stat().st_size / 1024:6.1f} КБ")


if __name__ == "__main__":
    main()
