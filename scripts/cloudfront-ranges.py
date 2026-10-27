#!/usr/bin/env python3
"""CloudFront 오리진 쪽 IP 대역 스냅샷 갱신: python3 scripts/cloudfront-ranges.py > config/cloudfront-origin-facing.json

백엔드 HTTPS 리스너(crates/core/src/edge_tls.rs)는 이 스냅샷으로 시작하고, 떠 있는 동안 AWS의
ip-ranges.json을 6시간마다 다시 받는다. 스냅샷은 그 첫 응답 전·AWS에 닿지 않을 때만 쓰인다.
"""
import json
import sys
import urllib.request

URL = "https://ip-ranges.amazonaws.com/ip-ranges.json"
SERVICE = "CLOUDFRONT_ORIGIN_FACING"

with urllib.request.urlopen(URL, timeout=30) as r:
    data = json.load(r)
v4 = sorted({p["ip_prefix"] for p in data["prefixes"] if p["service"] == SERVICE})
v6 = sorted({p["ipv6_prefix"] for p in data["ipv6_prefixes"] if p["service"] == SERVICE})
if len(v4) < 10:
    sys.exit(f"{SERVICE}: IPv4 대역이 {len(v4)}개뿐이에요 — 받은 파일을 확인하세요")
json.dump({"source": URL, "service": SERVICE, "syncToken": data["syncToken"],
           "createDate": data["createDate"], "prefixes": v4 + v6}, sys.stdout, indent=1)
sys.stdout.write("\n")
