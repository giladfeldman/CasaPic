"""Makes the test library the core integration tests read (crates/core/tests/library.rs).

Computer-generated pictures only (colour gradients with noise, no real photos), so it can be
remade on any machine: two month folders of 12 photos each, 4000 x 3000 JPEGs with camera data
like a camera dump (date taken, Make "CasaPicLab", an embedded preview picture).

    python scripts/make_testlib.py            # writes tmp/testlib (gitignored)
    python scripts/make_testlib.py OUT_DIR

Needs Pillow (pip install pillow). The camera data is written here by hand, as cameras write it:
the Python library piexif leaves out the closing link of the EXIF directory, which strict readers
reject.
"""
import io
import random
import sys
from pathlib import Path

import struct
from PIL import Image, ImageFilter

W, H = 4000, 3000
PER_MONTH = 12


def base_picture(seed: int) -> Image.Image:
    """A smooth two-colour gradient with soft blobs and fine noise (decodes like a photo)."""
    rnd = random.Random(seed)
    small = Image.new("RGB", (80, 60))
    c1 = [rnd.randint(20, 235) for _ in range(3)]
    c2 = [rnd.randint(20, 235) for _ in range(3)]
    px = small.load()
    for y in range(60):
        for x in range(80):
            t = (x / 79 + y / 59) / 2
            px[x, y] = tuple(int(a + (b - a) * t + rnd.randint(-25, 25)) for a, b in zip(c1, c2))
    img = small.resize((W, H), Image.BICUBIC).filter(ImageFilter.GaussianBlur(4))
    noise = Image.effect_noise((W, H), 18).convert("RGB")
    return Image.blend(img, noise, 0.08)


def camera_data(when: str, thumb: bytes) -> bytes:
    """A big-endian EXIF block: IFD0 (Make, DateTime, link to EXIF), the EXIF directory
    (DateTimeOriginal, DateTimeDigitized) and IFD1 (the embedded preview), each directory
    closed by its 4-byte link, as the TIFF format requires."""
    make, date = b"CasaPicLab\0", when.encode() + b"\0"

    def ifd(entries, data_at, nxt):
        """entries: (tag, type, count, bytes or int); returns directory bytes and its data"""
        head, data = struct.pack(">H", len(entries)), b""
        for tag, typ, count, val in sorted(entries):
            if isinstance(val, int):
                head += struct.pack(">HHII", tag, typ, count, val)
            else:
                head += struct.pack(">HHII", tag, typ, count, data_at + len(data))
                data += val + (b"\0" if len(val) % 2 else b"")
        return head + struct.pack(">I", nxt), data

    def padded(b):
        return len(b) + len(b) % 2
    def size(n):
        return 2 + 12 * n + 4

    ifd0_at = 8
    ifd0_data_at = ifd0_at + size(3)
    exif_at = ifd0_data_at + padded(make) + padded(date)
    exif_data_at = exif_at + size(2)
    ifd1_at = exif_data_at + 2 * padded(date)
    thumb_at = ifd1_at + size(2)
    d0, x0 = ifd([(0x010F, 2, len(make), make), (0x0132, 2, len(date), date), (0x8769, 4, 1, exif_at)], ifd0_data_at, ifd1_at)
    assert ifd0_data_at + len(x0) == exif_at
    d1, x1 = ifd([(0x9003, 2, len(date), date), (0x9004, 2, len(date), date)], exif_data_at, 0)
    assert exif_data_at + len(x1) == ifd1_at
    d2, _ = ifd([(0x0201, 4, 1, thumb_at), (0x0202, 4, 1, len(thumb))], thumb_at, 0)
    return b"Exif\0\0" + b"MM\0*" + struct.pack(">I", ifd0_at) + d0 + x0 + d1 + x1 + d2 + thumb


def main() -> None:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent / "tmp" / "testlib"
    bases = [base_picture(s) for s in range(4)]
    n = 0
    # numbered like the original camera dump: photo i is in month (i % 12) + 1, so January
    # holds IMG_0012, IMG_0024, ... and February IMG_0001, IMG_0013, ...
    for month in (1, 2):
        folder = out / f"2015-{month:02d} Trip"
        folder.mkdir(parents=True, exist_ok=True)
        for k in range(PER_MONTH):
            i = 12 * k + month - 1 if month > 1 else 12 * (k + 1)
            n += 1
            pic = bases[i % len(bases)]
            thumb = io.BytesIO()
            pic.resize((160, 120)).save(thumb, "JPEG", quality=80)
            when = f"2015:{month:02d}:{(i % 27) + 1:02d} 10:{i % 60:02d}:00"
            exif = camera_data(when, thumb.getvalue())
            pic.save(folder / f"IMG_{i:04d}.jpg", "JPEG", quality=90, exif=exif)
    print(f"made {n} photos in {out}")


if __name__ == "__main__":
    main()
