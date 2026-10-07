FROM rust:1.98.1-slim-trixie AS chef
# 빌드·실행 모두 Debian 13(trixie) slim 기반: 빌드는 rust slim-trixie, 실행은 debian:13-slim
# 의존성은 cargo-chef로 따로 한 층에 빌드해, 우리 코드만 바뀌면 그 층을 캐시에서 그대로 쓴다
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /build
COPY deploy/build-target.sh /usr/local/bin/audeniq-build-target
RUN chmod 0755 /usr/local/bin/audeniq-build-target

FROM chef AS plan
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
# release(기본): LTO까지 한 운영 빌드. fast: 테스트 서버용 (Cargo.toml [profile.fast]). dev: CI 스모크용
ARG CARGO_PROFILE=release
# BuildKit selects the matching Rust/Debian image for each target platform.
# Empty TARGET_CPU is portable: x86-64 on amd64, generic ARMv8-A on arm64.
# Opt in to x86-64-v3 or neoverse-v2 (AWS Graviton4) for tuned images.
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
    && cp "target/$dir/audeniq-api" "target/$dir/audeniq-worker" "target/$dir/audeniq-migrate" "target/$dir/audeniq-admin" /out/

# The AUDENIQ media engine (probe, lossless FLAC conversion, QC decode,
# fingerprint windows) at the commit pinned in deploy/audeniq-qc.rev. It is a
# separate binary so uploads are parsed only in its sandboxed child process.
FROM chef AS engine
ARG TARGETARCH
ARG TARGET_CPU=
COPY deploy/audeniq-qc.rev deploy/install-audeniq-qc.sh /engine/
RUN audeniq-build-target "$TARGETARCH" "$TARGET_CPU" sh /engine/install-audeniq-qc.sh /engine/root
FROM debian:13-slim
# Audio probing, lossless FLAC conversion, Stage 1 QC and fingerprinting run
# in audeniq-qc (built above); FFmpeg is no longer part of the runtime image.
# openssh-client: SFTP partner delivery (DDEX drops) runs `sftp -b -` with a
# pinned known_hosts file (crate::transport::SftpTransport).
# /var/tmp/audeniq is the worker TMPDIR. A new named volume copies this
# ownership, so UID 10001 can write downloaded masters for QC.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates openssh-client libxml2-utils libimage-exiftool-perl tesseract-ocr tesseract-ocr-eng tesseract-ocr-kor zbar-tools python3-pil poppler-utils clamav-daemon clamav-freshclam && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 audeniq && useradd --uid 10001 --gid 10001 --no-create-home audeniq \
    && install -d -o 10001 -g 10001 -m 700 /var/tmp/audeniq \
    && install -d -o 10001 -g 10001 -m 755 /var/lib/clamav /run/audeniq-av
COPY --from=build /out/ /usr/local/bin/
COPY --from=engine /engine/root/bin/audeniq-qc /usr/local/bin/audeniq-qc
COPY --from=build /meta/ /usr/local/share/audeniq/
COPY deploy/sanitize-upload.py /usr/local/lib/audeniq/sanitize-upload.py
COPY deploy/update-antivirus.py /usr/local/lib/audeniq/update-antivirus.py
COPY deploy/clamd.conf deploy/freshclam.conf /usr/local/share/audeniq/
USER 10001:10001
ENV RUST_LOG=info
CMD ["audeniq-api"]
