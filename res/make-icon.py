# Draws des-k.ico, the exe's icon: the title bar mark (ui.rs `mark`, colors `MARK`), four
# rounded squares, at each size Windows asks for. Run it again after changing the mark:
#   python res/make-icon.py
# Each size is drawn on its own pixel grid, so the small ones stay sharp instead of being
# shrunk from the big one.
import io, struct
from pathlib import Path
from PIL import Image, ImageDraw

MARK = [0xB93C87, 0xE53935, 0x8E3FD9, 0xB93C87]  # top left, top right, bottom left, bottom right
SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
SS = 8  # supersampling, for smooth corners


def draw(s):
    u = s / 16  # the mark is 16 dip: 7 px squares, 2 px gap, 2 px corners
    gap = max(1, round(2 * u))
    q = (s - gap) // 2
    off = (s - 2 * q - gap) // 2
    big = Image.new("RGBA", (s * SS, s * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    for i, (ox, oy) in enumerate([(off, off), (off + q + gap, off), (off, off + q + gap), (off + q + gap, off + q + gap)]):
        c = MARK[i]
        box = [ox * SS, oy * SS, (ox + q) * SS - 1, (oy + q) * SS - 1]
        d.rounded_rectangle(box, radius=2 * u * SS, fill=(c >> 16, c >> 8 & 0xFF, c & 0xFF, 255))
    return big.resize((s, s), Image.LANCZOS)


def ico(images):
    # ICO with PNG entries (read by Windows since Vista, and by rc.exe).
    pngs = []
    for im in images:
        b = io.BytesIO()
        im.save(b, "PNG")
        pngs.append(b.getvalue())
    out = struct.pack("<HHH", 0, 1, len(images))
    at = 6 + 16 * len(images)
    for im, png in zip(images, pngs):
        w = im.width % 256  # 0 means 256
        out += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(png), at)
        at += len(png)
    return out + b"".join(pngs)


here = Path(__file__).parent
(here / "des-k.ico").write_bytes(ico([draw(s) for s in SIZES]))
draw(256).save(here.parent / "target" / "icon-256.png") if (here.parent / "target").is_dir() else None
