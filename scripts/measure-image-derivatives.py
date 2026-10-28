#!/usr/bin/env python3
"""Local-only measurement of the shared codec. Requires Pillow; never opens DB/storage.
Build: cargo build --offline -p audeniq-core --bin audeniq-image-encode
Run: python3 scripts/measure-image-derivatives.py
This is NOT a photographic/lossy/AVIF quality benchmark.
"""
import argparse
import hashlib
import pathlib
import subprocess
import tempfile
import time
from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--encoder", type=pathlib.Path,
                    default=ROOT / "target/debug/audeniq-image-encode")
BINARY = parser.parse_args().encoder.resolve()
SAMPLES = [
    "web/landing/public/assets/social-preview.png",
    "web/survey/public/assets/social-preview.png",
    "web/studio/app/public/static/brands/canva.png",
]
print("| Checkout graphic | Original bytes | WebP bytes | Saving | Encode ms | PSNR | SSIM | Publish? |")
print("|---|---:|---:|---:|---:|---:|---:|---|")
with tempfile.TemporaryDirectory(prefix="audeniq-image-measure-") as tmp:
    for n, relative in enumerate(SAMPLES):
        source = ROOT / relative
        output = pathlib.Path(tmp) / f"{n}.webp"
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        start = time.perf_counter()
        subprocess.run([str(BINARY), "sandbox-webp", str(source), str(output), digest, "8192"],
                       check=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
        elapsed = (time.perf_counter() - start) * 1000
        with Image.open(source) as a, Image.open(output) as b:
            a, b = a.convert("RGBA"), b.convert("RGBA")
            assert a.size == b.size and a.tobytes() == b.tobytes(), "pixel mismatch"
        original, encoded = source.stat().st_size, output.stat().st_size
        useful = encoded <= original - 1024 and encoded * 10 <= original * 9
        # Identical pixels imply MSE=0, PSNR=+infinity and SSIM=1 exactly.
        print(f"| `{relative}` | {original} | {encoded} | {100*(1-encoded/original):.2f}% | {elapsed:.1f} | ∞ | 1 | {'yes' if useful else 'skip'} |")
