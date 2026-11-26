"""Render the app icon and the installer artwork from crates/phoecord-installer-gui/assets/icon/icon.svg.

Needs `rsvg-convert` (librsvg) and Pillow; on macOS also `iconutil` for the .icns. Run it after the
SVG changes and commit the results: packaging only reads the files this writes.

    python scripts/package/make_art.py
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[2]
GUI = ROOT / "crates/phoecord-installer-gui/assets"
SVG = GUI / "icon/icon.svg"
FONT = GUI / "fonts/Onest-SemiBold.ttf"
FONT_REGULAR = GUI / "fonts/Onest-Regular.ttf"
OUT = GUI / "installer"

# The window's palette (src/theme.rs).
BG = (0x0E, 0x0A, 0x0B)
TEXT = (0xF6, 0xED, 0xEE)
MUTED = (0xA8, 0x94, 0x9A)
ACCENT = (0xA3, 0x18, 0x2F)


def render(size: int, tmp: Path) -> Image.Image:
    out = tmp / f"icon-{size}.png"
    subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), str(SVG), "-o", str(out)], check=True
    )
    return Image.open(out).convert("RGBA")


def glow(size: tuple[int, int], center: tuple[int, int], radius: int, alpha: int) -> Image.Image:
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    x, y = center
    d.ellipse((x - radius, y - radius, x + radius, y + radius), fill=(*ACCENT, alpha))
    return layer.filter(ImageFilter.GaussianBlur(radius / 2))


def canvas(w: int, h: int, mark_at: tuple[int, int], mark: int, tmp: Path) -> Image.Image:
    im = Image.new("RGBA", (w, h), (*BG, 255))
    im = Image.alpha_composite(im, glow((w, h), mark_at, int(mark * 1.1), 150))
    icon = render(mark, tmp)
    im.alpha_composite(icon, (mark_at[0] - mark // 2, mark_at[1] - mark // 2))
    return im


def text(im: Image.Image, xy, s: str, size: int, fill, font=FONT, anchor="la") -> None:
    ImageDraw.Draw(im).text(xy, s, font=ImageFont.truetype(str(font), size), fill=fill, anchor=anchor)


def bmp(im: Image.Image, name: str) -> None:
    im.convert("RGB").save(OUT / name, "BMP")


def main() -> int:
    if not shutil.which("rsvg-convert"):
        print("rsvg-convert is missing (brew install librsvg / apt install librsvg2-bin)")
        return 1
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        # App icons.
        render(256, tmp).save(GUI / "icon/icon-256.png")
        render(1024, tmp).save(GUI / "icon/icon-1024.png")
        render(1024, tmp).save(
            GUI / "icon/icon.ico",
            sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
        )
        if shutil.which("iconutil"):
            iconset = tmp / "icon.iconset"
            iconset.mkdir()
            for n in (16, 32, 128, 256, 512):
                render(n, tmp).save(iconset / f"icon_{n}x{n}.png")
                render(n * 2, tmp).save(iconset / f"icon_{n}x{n}@2x.png")
            subprocess.run(
                ["iconutil", "-c", "icns", str(iconset), "-o", str(GUI / "icon/icon.icns")],
                check=True,
            )

        # NSIS: welcome / finish sidebar (164x314) and the page header (150x57).
        side = canvas(164, 314, (82, 120), 72, tmp)
        text(side, (82, 186), "PhoeCord", 20, TEXT, anchor="ma")
        text(side, (82, 214), "Discord, your way", 11, MUTED, FONT_REGULAR, anchor="ma")
        bmp(side, "nsis-sidebar.bmp")
        head = canvas(150, 57, (124, 28), 34, tmp)
        bmp(head, "nsis-header.bmp")

        # DMG window background (640x400): the app on the left, Applications on the right. Finder
        # draws the icon labels in black, so this one is light.
        paper, ink, soft = (0xFB, 0xF6, 0xF6), (0x2A, 0x14, 0x19), (0x8A, 0x6E, 0x74)
        dmg = Image.new("RGBA", (640, 400), (*paper, 255))
        dmg = Image.alpha_composite(dmg, glow((640, 400), (180, 190), 110, 46))
        text(dmg, (320, 46), "Install PhoeCord", 22, ink, anchor="ma")
        text(dmg, (320, 80), "Drag PhoeCord into Applications", 13, soft, FONT_REGULAR, anchor="ma")
        d = ImageDraw.Draw(dmg)
        d.line((266, 190, 370, 190), fill=(*ACCENT, 255), width=2)
        d.polygon([(370, 184), (381, 190), (370, 196)], fill=(*ACCENT, 255))
        dmg.convert("RGB").save(OUT / "dmg-background.png")
    print("written:", ", ".join(sorted(p.name for p in OUT.iterdir())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
