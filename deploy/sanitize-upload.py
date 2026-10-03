#!/usr/bin/env python3
"""Create static derivatives. Called only inside the mandatory parser sandbox."""
import io
import pathlib
import subprocess
import sys
import warnings

from PIL import Image, ImageCms, ImageOps

Image.MAX_IMAGE_PIXELS = 40_000_000
warnings.simplefilter("error", Image.DecompressionBombWarning)
MAX_BYTES = 20 * 1024 * 1024


def pixels(path):
    with Image.open(path) as source:
        if source.format not in {"JPEG", "PNG"} or getattr(source, "n_frames", 1) != 1:
            raise ValueError("static JPEG/PNG required")
        source.load()
        image = ImageOps.exif_transpose(source)
        if image.width > 8000 or image.height > 8000:
            raise ValueError("image dimensions exceeded")
        profile = image.info.get("icc_profile")
        if profile:
            image = ImageCms.profileToProfile(
                image, ImageCms.ImageCmsProfile(io.BytesIO(profile)),
                ImageCms.createProfile("sRGB"), outputMode="RGB")
        else:
            image = image.convert("RGB")
        # A new pixel-only object cannot carry EXIF, comments, text chunks,
        # scripts, ICC blobs, appended files or other source metadata.
        clean = Image.new("RGB", image.size)
        clean.paste(image)
        return clean


def image_only_pdf(paths, dst):
    """Write a PDF with only pages, JPEG XObjects and drawing streams."""
    offsets = [0]
    total_pixels = 0
    with dst.open("wb") as out:
        out.write(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")

        def obj(number, data):
            offsets.append(out.tell())
            out.write(f"{number} 0 obj\n".encode() + data + b"\nendobj\n")
            if out.tell() > MAX_BYTES:
                raise ValueError("document derivative exceeded limit")

        obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        kids = " ".join(f"{3 + i * 3} 0 R" for i in range(len(paths)))
        obj(2, f"<< /Type /Pages /Count {len(paths)} /Kids [{kids}] >>".encode())
        for i, path in enumerate(paths):
            image = pixels(path)
            w, h = image.size
            total_pixels += w * h
            if total_pixels > 64_000_000:
                raise ValueError("document pixel budget exceeded")
            jpeg = io.BytesIO()
            image.save(jpeg, "JPEG", quality=85)
            data = jpeg.getvalue()
            n = 3 + i * 3
            # Preserve aspect ratio; all pages have a constant maximum size.
            scale = 792 / max(w, h)
            pw, ph = w * scale, h * scale
            obj(n, (f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.4f} {ph:.4f}] "
                    f"/Resources << /XObject << /Im0 {n+1} 0 R >> >> /Contents {n+2} 0 R >>").encode())
            obj(n+1, (f"<< /Type /XObject /Subtype /Image /Width {w} /Height {h} "
                      f"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode "
                      f"/Length {len(data)} >>\nstream\n").encode() + data + b"\nendstream")
            drawing = f"q {pw:.4f} 0 0 {ph:.4f} 0 0 cm /Im0 Do Q".encode()
            obj(n+2, f"<< /Length {len(drawing)} >>\nstream\n".encode() + drawing + b"\nendstream")
        start = out.tell()
        out.write(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
        for offset in offsets[1:]:
            out.write(f"{offset:010} 00000 n \n".encode())
        out.write(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n".encode())


def sanitize(src, dst, mime):
    if mime in {"image/png", "image/jpeg"}:
        clean = pixels(src)
        if mime == "image/png":
            clean.save(dst, "PNG", optimize=False)
        else:
            clean.save(dst, "JPEG", quality=95, subsampling=0)
    elif mime == "application/pdf":
        info = subprocess.run(["pdfinfo", str(src)], capture_output=True, timeout=15, check=True)
        if len(info.stdout) > 64 * 1024:
            raise ValueError("document info exceeded limit")
        fields = dict(line.split(":", 1) for line in info.stdout.decode("utf-8", "replace").splitlines() if ":" in line)
        pages = int(fields.get("Pages", "0"))
        if not 1 <= pages <= 32 or not fields.get("Encrypted", "").strip().startswith("no"):
            raise ValueError("encrypted or oversized document")
        prefix = dst.parent / "raster"
        subprocess.run(["pdftoppm", "-q", "-jpeg", "-r", "110", "-scale-to", "2048",
                        "-f", "1", "-l", str(pages), str(src), str(prefix)],
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=90, check=True)
        rasters = sorted(dst.parent.glob("raster-*.jpg"))
        if len(rasters) != pages:
            raise ValueError("incomplete document rasterization")
        image_only_pdf(rasters, dst)
    else:
        raise ValueError("unsupported type")
    if not 0 < dst.stat().st_size <= MAX_BYTES:
        raise ValueError("output exceeded limit")


if __name__ == "__main__":
    try:
        sanitize(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3])
    except Exception:
        # Parser diagnostics can contain private document text or source paths.
        print("upload sanitization failed", file=sys.stderr)
        sys.exit(1)
