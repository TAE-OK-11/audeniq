#!/usr/bin/env python3
"""Summarize --nocapture ceremony_latency_sections logs (first 5 samples warm up)."""
import collections
import json
import math
import re
import statistics
import sys


def summarize(path):
    samples = collections.defaultdict(list)
    sample = -1
    with open(path, encoding="utf-8") as log:
        for line in log:
            marker = re.search(r"LATENCY_SAMPLE (\d+)", line)
            if marker:
                sample = int(marker.group(1))
            if sample < 5 or not line.startswith("{"):
                continue
            event = json.loads(line)
            if event.get("target") != "passkey_timing":
                continue
            fields = event["fields"]
            samples[(fields["ceremony"], fields["section"])].append(fields["elapsed_us"])
    return {f"{ceremony}/{section}": {
        "n": len(values),
        "p50_us": statistics.median(values),
        "p95_us": sorted(values)[math.ceil(len(values) * .95) - 1],
    } for (ceremony, section), values in sorted(samples.items())}


if __name__ == "__main__":
    print(json.dumps({label: summarize(path) for label, path in
                      (arg.split("=", 1) for arg in sys.argv[1:])}, indent=2))
