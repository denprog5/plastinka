"""Картинки из source/ (сгенерированы Codex) -> assets/ (встраиваются в exe; после запуска пересоберите).

- source/panel-bg.png  -> assets/web/bg.webp      фон карточки
- source/glitch.png    -> assets/web/glitch.webp  красная глитч-полоса: чёрный фон -> прозрачность
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

# Глитч: яркость -> альфа, цвет восстанавливаем «разделением» на альфу (unpremultiply),
# чтобы на прозрачном фоне OBS полоса выглядела так же, как на чёрном.
g = np.asarray(Image.open(SRC / "glitch.png").convert("RGB")).astype(np.float32) / 255
a = g.max(axis=2)
a = np.clip((a - 0.04) / 0.96, 0, 1)  # убрать почти-чёрный шум фона
rgb = np.where(a[..., None] > 0, np.clip(g / np.maximum(a[..., None], 1e-3), 0, 1), 0)
out = np.dstack([rgb, a])
img = Image.fromarray((out * 255).astype(np.uint8), "RGBA")
img = img.crop(img.getbbox())
img = img.resize((1200, max(1, round(img.height * 1200 / img.width))), Image.LANCZOS)
img.save(WEB / "glitch.webp", "WEBP", quality=85, method=6)

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
