FROM rust:1.98.1-slim-trixie@sha256:4cd829461bd5c4d511c32e269da9cb8929223b666519d8004e35fc8d1d771ab7 AS chef
# 빌드·실행 모두 Debian 13(trixie) slim 기반: 빌드는 rust slim-trixie, 실행은 debian:13-slim
# 의존성은 cargo-chef로 따로 한 층에 빌드해, 우리 코드만 바뀌면 그 층을 캐시에서 그대로 쓴다
# make: tikv-jemalloc-sys builds jemalloc with its configure/make (the slim image has gcc only).
RUN apt-get update && apt-get install -y --no-install-recommends make && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /build
COPY deploy/build-target.sh /usr/local/bin/audeniq-build-target
COPY deploy/with-github-token.sh /usr/local/bin/audeniq-with-github-token
RUN chmod 0755 /usr/local/bin/audeniq-build-target /usr/local/bin/audeniq-with-github-token

FROM chef AS plan
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
# release(기본): LTO까지 한 운영 빌드. fast: 테스트 서버용 (Cargo.toml [profile.fast]). dev: CI 스모크용
ARG CARGO_PROFILE=release
# BuildKit selects the matching Rust/Debian image for each target platform.
# Empty TARGET_CPU is portable: x86-64 on amd64, generic ARMv8-A on arm64.
# Opt in to x86-64-v3, neoverse-v2 (AWS Graviton4), ampere1a (Oracle Cloud A4,
# AmpereOne M) or neoverse-n1 (Oracle Cloud Ampere A1, AWS Graviton2) for tuned images.
ARG TARGETARCH
ARG TARGET_CPU=
COPY --from=plan /build/recipe.json recipe.json
RUN audeniq-build-target "$TARGETARCH" "$TARGET_CPU" \
    cargo chef cook --locked --profile "$CARGO_PROFILE" -p audeniq-core --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY config ./config
COPY migrations ./migrations
RUN AUDENIQ_BUILD_METADATA_DIR=/meta audeniq-build-target "$TARGETARCH" "$TARGET_CPU" \
    cargo build --locked --profile "$CARGO_PROFILE" -p audeniq-core --bins \
    && mkdir -p /out /meta \
    && case "$TARGETARCH" in amd64) triple=x86_64-unknown-linux-gnu ;; arm64) triple=aarch64-unknown-linux-gnu ;; esac \
    && profile_dir=$([ "$CARGO_PROFILE" = dev ] && echo debug || echo "$CARGO_PROFILE") \
    && dir="$triple/$profile_dir" \
    && cp "target/$dir/audeniq-api" "target/$dir/audeniq-worker" "target/$dir/audeniq-migrate" "target/$dir/audeniq-admin" "target/$dir/audeniq-secrets" "target/$dir/audeniq-image-encode" /out/

# The AUDENIQ media engine at the commit pinned in deploy/audeniq-music.rev:
# `audeniq-music qc` (probe, lossless FLAC conversion, QC decode, fingerprint
# windows), `audeniq-music photo` (cover metadata, QR, OCR, upload
# sanitization; replaces ExifTool, ZBar, Tesseract and the Pillow/Poppler
# sanitizer) and `audeniq-music video` (music-video probe and delivery
# checks; replaces FFprobe). It is a separate binary so uploads are parsed only in its
# sandboxed child process.
FROM chef AS engine
ARG TARGETARCH
ARG TARGET_CPU=
COPY deploy/audeniq-music.rev deploy/install-audeniq-music.sh /engine/
# CI passes a read-only token as the BuildKit secret `github_token` (never
# stored in a layer; see with-github-token.sh).
RUN --mount=type=secret,id=github_token \
    audeniq-build-target "$TARGETARCH" "$TARGET_CPU" \
    audeniq-with-github-token sh /engine/install-audeniq-music.sh /engine/root
FROM debian:13-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f
# Audio probing, lossless FLAC conversion, Stage 1 QC and fingerprinting
# (`audeniq-music qc`) and image metadata, QR, OCR and upload sanitization
# (`audeniq-music photo`) run in audeniq-music (built above); FFmpeg is no
# longer part of the runtime image. No
# Python: the antivirus updater and the parser sandbox self-test are part of
# audeniq-admin (`antivirus-update`, `upload-sandbox-check`).
# openssh-client: SFTP partner delivery (DDEX drops) runs `sftp -b -` with a
# pinned known_hosts file (crate::transport::SftpTransport).
# /var/tmp/audeniq is the worker TMPDIR. A new named volume copies this
# ownership, so UID 10001 can write downloaded masters for QC.
# /var/lib/audeniq-tls: the api's Let's Encrypt account and certificates
# (crate::edge_acme, compose volume api_tls), private to UID 10001.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates openssh-client libxml2-utils clamav-daemon clamav-freshclam && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 audeniq && useradd --uid 10001 --gid 10001 --no-create-home audeniq \
    && install -d -o 10001 -g 10001 -m 700 /var/tmp/audeniq /var/lib/audeniq-tls \
    && install -d -o 10001 -g 10001 -m 755 /var/lib/clamav /run/audeniq-av
COPY --from=build /out/ /usr/local/bin/
COPY --from=engine /engine/root/bin/audeniq-music /usr/local/bin/audeniq-music
COPY --from=build /meta/ /usr/local/share/audeniq/
COPY deploy/clamd.conf deploy/freshclam.conf /usr/local/share/audeniq/
USER 10001:10001
ENV RUST_LOG=info
CMD ["audeniq-api"]
