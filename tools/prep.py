"""Картинки из source/ (сгенерированы Codex) -> assets/ (встраиваются в exe; после запуска пересоберите).

- source/panel-bg.png  -> assets/web/bg.webp      фон карточки
- source/glitch.png    -> assets/web/glitch.webp  красная глитч-полоса: чёрный фон -> прозрачность
- source/imperium-bg.png, imperium-crest.png -> assets/web/imperium-*.webp  тема imperium
- source/neon-bg.png   -> assets/web/neon-bg.webp  тема neon
- source/icon-src.png  -> assets/icon.ico         иконка exe и трея (играет)
                          assets/tray-idle.ico    серая — ничего не играет
                          assets/web/vinyl.webp   заглушка, если у трека нет обложки
                          assets/wizard-small*.png  картинка мастера установки

Запуск:  python tools/prep.py
"""
from pathlib import Path

import numpy as np
from PIL import Image, ImageEnhance

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "source"
WEB = ROOT / "assets" / "web"
WEB.mkdir(parents=True, exist_ok=True)

# Фон: карточка 560x136 css px, рисуем с запасом x2 для чёткости на 1440p.
bg = Image.open(SRC / "panel-bg.png").convert("RGB")
bg = bg.resize((1120, 373), Image.LANCZOS)
bg.save(WEB / "bg.webp", "WEBP", quality=82, method=6)


def black_to_alpha(path, lo, hi):
    """Картинка на чёрном -> RGBA: яркость от lo до hi становится альфой, цвет восстанавливаем
    «разделением» на альфу (unpremultiply), чтобы на прозрачном фоне OBS всё выглядело как на чёрном."""
    g = np.asarray(Image.open(path).convert("RGB")).astype(np.float32) / 255
    a = np.clip((g.max(axis=2) - lo) / (hi - lo), 0, 1)
    rgb = np.where(a[..., None] > 0, np.clip(g / np.maximum(a[..., None], 1e-3), 0, 1), 0)
    img = Image.fromarray((np.dstack([rgb, a]) * 255).astype(np.uint8), "RGBA")
    return img.crop(img.getbbox())


# Глитч (тема glitch): почти-чёрный шум фона отрезаем.
img = black_to_alpha(SRC / "glitch.png", 0.04, 1.0)
img = img.resize((1200, max(1, round(img.height * 1200 / img.width))), Image.LANCZOS)
img.save(WEB / "glitch.webp", "WEBP", quality=85, method=6)

# Тема imperium: плита и крылатый череп (на карточке ~36 px, держим x4 запас).
Image.open(SRC / "imperium-bg.png").convert("RGB").resize((1120, 256), Image.LANCZOS).save(
    WEB / "imperium-bg.webp", "WEBP", quality=82, method=6)
# Тёмные гравировки золота должны остаться непрозрачными — порог альфы низкий.
crest = black_to_alpha(SRC / "imperium-crest.png", 0.05, 0.30)
crest = crest.resize((160, max(1, round(crest.height * 160 / crest.width))), Image.LANCZOS)
crest.save(WEB / "imperium-crest.webp", "WEBP", quality=90, method=6)

# Тема neon: сетка и свечение по нижнему краю.
Image.open(SRC / "neon-bg.png").convert("RGB").resize((1120, 373), Image.LANCZOS).save(
    WEB / "neon-bg.webp", "WEBP", quality=82, method=6)

# Иконки.
icon = Image.open(SRC / "icon-src.png").convert("RGBA")
icon = icon.crop(icon.getbbox())
side = max(icon.size)
sq = Image.new("RGBA", (side, side))
sq.paste(icon, ((side - icon.width) // 2, (side - icon.height) // 2))
sizes = [(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 256)]
sq.save(ROOT / "assets" / "icon.ico", sizes=sizes)
gray = sq.convert("LA").convert("RGBA")
gray.putalpha(sq.getchannel("A"))
gray = ImageEnhance.Brightness(gray).enhance(1.6)
gray.save(ROOT / "assets" / "tray-idle.ico", sizes=sizes)
sq.resize((256, 256), Image.LANCZOS).save(WEB / "vinyl.webp", "WEBP", quality=85, method=6)
# Картинка в углу мастера установки (Inno Setup): 55 px и 110 px для 200% масштаба.
for px, name in ((55, "wizard-small.png"), (110, "wizard-small-2x.png")):
    sq.resize((px, px), Image.LANCZOS).save(ROOT / "assets" / name)

for p in sorted([*WEB.iterdir(), ROOT / "assets" / "icon.ico", ROOT / "assets" / "tray-idle.ico"]):
    print(f"{p.relative_to(ROOT)}  {p.stat().st_size // 1024} КБ")
