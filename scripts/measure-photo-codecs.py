#!/usr/bin/env python3
"""Offline photo grid: supply NAME=JPEG/PNG paths and a locally built Rust probe.
Requires Pillow, NumPy, SciPy, and existing libheif.so.1 (AVIF measurement only).
No network, database, object storage, external codec executables or credentials.
Input files are decoded exactly as supplied; resizing/normalization is explicit
preparation outside this script. PSNR is RGB; SSIM uses Rec.709 luma, 11x11 box
windows, reflected borders, population variance, C1=6.5025 and C2=58.5225.
"""
import argparse
import hashlib
import json
import math
import pathlib
import subprocess
import time

import numpy as np
from PIL import Image
from scipy.ndimage import uniform_filter
from measure_avif import load


def metrics(a, b):
    assert a.shape == b.shape, "decoded dimensions/channels differ"
    a, b = a[:, :, :3].astype(float), b[:, :, :3].astype(float)
    mse = np.mean((a - b) ** 2)
    x, y = a @ np.array([.2126, .7152, .0722]), b @ np.array([.2126, .7152, .0722])
    ux, uy = uniform_filter(x, 11), uniform_filter(y, 11)
    vx, vy = uniform_filter(x*x, 11) - ux*ux, uniform_filter(y*y, 11) - uy*uy
    cov = uniform_filter(x*y, 11) - ux*uy
    score = np.mean(((2*ux*uy+6.5025)*(2*cov+58.5225)) /
                    ((ux*ux+uy*uy+6.5025)*(vx+vy+58.5225)))
    return {"psnr": round(10*math.log10(255**2/mse), 3) if mse else "infinity",
            "ssim": round(float(score), 6)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=pathlib.Path, required=True)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    parser.add_argument("sources", nargs="+", help="NAME=path to input JPEG/PNG")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    rows = []
    grid = [("c", 85, 4), ("c", 90, 4), ("c", 95, 4),
            ("avif", 80, 8), ("avif", 90, 8), ("avif", 90, 6)]
    for item in args.sources:
        name, path = item.split("=", 1)
        assert name.replace("-", "").isalnum(), "use alphanumeric sample names"
        source = pathlib.Path(path)
        with Image.open(source) as im:
            a = np.asarray(im.convert("RGBA"))
        h, w, _ = a.shape
        raw = args.out / f"{name}.rgba"
        raw.write_bytes(a.tobytes())
        for codec, quality, speed in grid:
            out = args.out / f'{name}-{codec}-{quality}-{speed}.{ "avif" if codec == "avif" else "webp"}'
            start = time.monotonic()
            subprocess.run([str(args.probe.resolve()), str(raw.resolve()), str(w), str(h),
                            codec, str(quality), str(speed), str(out.resolve())],
                           check=True, capture_output=True, timeout=120)
            elapsed = time.monotonic() - start
            b = load(out) if codec == "avif" else np.asarray(Image.open(out).convert("RGBA"))
            Image.fromarray(b).save(out.with_suffix(".decoded.png"))
            original, size = source.stat().st_size, out.stat().st_size
            row = dict(sample=name, width=w, height=h, codec=codec, quality=quality,
                       speed=speed, original_bytes=original, bytes=size,
                       source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                       seconds=round(elapsed, 3), **metrics(a, b))
            row["publish"] = size > 0 and size <= original-1024 and size*10 <= original*9
            rows.append(row)
            print(json.dumps(row), flush=True)
            (args.out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")


if __name__ == "__main__":
    main()
