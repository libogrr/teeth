"""Tests for make_icon.py. Run with: python tools/test_make_icon.py"""

import sys
import tempfile
import unittest
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))
import make_icon  # noqa: E402

ALL_SIZES = {(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)}
WHITE = (255, 255, 255, 255)
BLACK = (0, 0, 0, 255)


class MakeIconTest(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.dir = Path(temp.name)
        self.master = self.dir / "teeth.png"
        self.out = self.dir / "out"
        self.out.mkdir()

    def write(self, path, size, color):
        Image.new("RGBA", size, color).save(path)

    def build(self):
        make_icon.build(self.master, self.out)

    def test_every_size_comes_from_the_one_master(self):
        self.write(self.master, (1024, 1024), WHITE)
        self.build()
        with Image.open(self.out / "teeth.ico") as ico:
            self.assertEqual(set(ico.info["sizes"]), ALL_SIZES)
            for s, _ in ALL_SIZES:
                frame = ico.ico.getimage((s, s)).convert("RGBA")
                self.assertEqual(frame.getpixel((s // 2, s // 2)), WHITE, s)
        with Image.open(self.out / "teeth-window.png") as window:
            self.assertEqual(window.size, (256, 256))
            self.assertEqual(window.convert("RGBA").getpixel((128, 128)), WHITE)

    def test_the_splash_mark_is_the_white_silhouette_alone(self):
        master = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
        master.paste(BLACK, (0, 0, 1024, 1024))
        master.paste(WHITE, (300, 200, 724, 824))
        master.paste((0, 0, 0, 0), (0, 0, 20, 20))
        master.save(self.master)
        self.build()
        with Image.open(self.out / "teeth-mark.png") as mark:
            mark = mark.convert("RGBA")
            w, h = mark.size
            self.assertEqual((w, h), (424 + 4, 624 + 4))
            for corner in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
                self.assertEqual(mark.getpixel(corner)[3], 0, corner)
            self.assertEqual(mark.getpixel((w // 2, h // 2)), WHITE)
            for r, g, b, a in mark.get_flattened_data() if hasattr(mark, "get_flattened_data") else mark.getdata():
                if a:
                    self.assertEqual((r, g, b), (255, 255, 255))

    def assert_refused(self):
        with self.assertRaises(SystemExit):
            self.build()
        self.assertEqual(list(self.out.iterdir()), [])

    def test_a_master_that_is_not_square_is_refused_and_nothing_is_written(self):
        self.write(self.master, (1024, 1100), WHITE)
        self.assert_refused()

    def test_a_master_without_a_white_silhouette_is_refused_and_nothing_is_written(self):
        self.write(self.master, (1024, 1024), BLACK)
        self.assert_refused()

    def test_a_master_under_1024_pixels_is_refused_and_nothing_is_written(self):
        self.write(self.master, (512, 512), WHITE)
        self.assert_refused()


if __name__ == "__main__":
    unittest.main()
