"""Builds the Windows icon of Teeth from its master.

Usage: python tools/make_icon.py [MASTER OUT_DIR]

MASTER is the clean silhouette, white on a black square with rounded corners
(assets/icon/teeth.png), square and at least 1024 pixels wide. On 7 November 2026 the user
chose this minimalist logo as the main one, at every size, over the textured tooth of the kit.

Writes, in OUT_DIR (assets/icon by default):
- teeth.ico, embedded in the executable by build.rs, with the sizes Windows asks for;
- teeth-window.png, the 256-pixel icon for the window and the taskbar;
- teeth-mark.png, the white silhouette alone on a transparent background, cut to its size
  with a 2-pixel margin, for the splash screen and the installer.

The results are kept in the repository, so building Teeth does not need Python.
"""

import sys
from pathlib import Path

from PIL import Image, ImageChops

SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
MIN_SIDE = 1024
ASSETS = Path(__file__).resolve().parent.parent / "assets" / "icon"


def load(path: Path) -> Image.Image:
    """The master at `path`, refused unless it is square and big enough."""
    image = Image.open(path).convert("RGBA")
    width, height = image.size
    if width != height or width < MIN_SIDE:
        raise SystemExit(
            f"{path}: hace falta un PNG cuadrado de al menos {MIN_SIDE} píxeles, y mide {width} × {height}."
        )
    return image


def build(master_path: Path, out_dir: Path) -> None:
    master = load(master_path)
    # Checked before anything is written, like the master itself.
    silhouette = mark(master, master_path)
    frames = [master.resize((s, s), Image.LANCZOS) for s in SIZES]
    # Pillow uses each given frame for its size instead of shrinking the first one.
    frames[-1].save(out_dir / "teeth.ico", format="ICO", sizes=[f.size for f in frames], append_images=frames[:-1])
    frames[-1].save(out_dir / "teeth-window.png", optimize=True)
    silhouette.save(out_dir / "teeth-mark.png", optimize=True)


def mark(master: Image.Image, path: Path) -> Image.Image:
    """The white of `master` alone: what is black or transparent there becomes transparent."""
    alpha = ImageChops.darker(master.getchannel("A"), master.convert("L"))
    white = Image.new("RGBA", master.size, (255, 255, 255, 0))
    white.putalpha(alpha)
    box = alpha.getbbox()
    if box is None:
        raise SystemExit(f"{path}: no tiene ninguna silueta blanca.")
    left, top, right, bottom = box
    margin = 2
    return white.crop((left - margin, top - margin, right + margin, bottom + margin))


def main(argv: list[str]) -> None:
    if len(argv) == 2:
        master, out_dir = (Path(a) for a in argv)
    elif not argv:
        master, out_dir = ASSETS / "teeth.png", ASSETS
    else:
        raise SystemExit(__doc__)
    build(master, out_dir)


if __name__ == "__main__":
    main(sys.argv[1:])
