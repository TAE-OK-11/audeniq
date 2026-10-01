FROM rust:1.98.1-slim-trixie AS chef
# 빌드·실행 모두 Debian 13(trixie) slim 기반: 빌드는 rust slim-trixie, 실행은 debian:13-slim
# 의존성은 cargo-chef로 따로 한 층에 빌드해, 우리 코드만 바뀌면 그 층을 캐시에서 그대로 쓴다
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /build

FROM chef AS plan
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
# release(기본): LTO까지 한 운영 빌드. fast: 테스트 서버용 (Cargo.toml [profile.fast]). dev: CI 스모크용
ARG CARGO_PROFILE=release
# CPU 기준선 (예: x86-64-v3 = AVX2·BMI2·FMA, Zen3/Haswell 이상). 비우면 일반 x86-64.
# 의존성 층(cook)과 본 빌드가 같은 값을 써야 캐시가 맞는다. 값은 이미지에 기록되고,
# deploy.sh가 서버 CPU가 그 기준을 지원하는지 먼저 확인한다.
ARG TARGET_CPU=
COPY --from=plan /build/recipe.json recipe.json
RUN if [ -n "$TARGET_CPU" ]; then export RUSTFLAGS="-C target-cpu=$TARGET_CPU"; fi \
    && cargo chef cook --locked --profile "$CARGO_PROFILE" -p audeniq-core --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY config ./config
COPY migrations ./migrations
RUN if [ -n "$TARGET_CPU" ]; then export RUSTFLAGS="-C target-cpu=$TARGET_CPU"; fi \
    && cargo build --locked --profile "$CARGO_PROFILE" -p audeniq-core --bins \
    && mkdir -p /out /meta \
    && printf '%s\n' "${TARGET_CPU:-x86-64}" > /meta/target-cpu \
    && dir=$([ "$CARGO_PROFILE" = dev ] && echo debug || echo "$CARGO_PROFILE") \
    && cp "target/$dir/audeniq-api" "target/$dir/audeniq-worker" "target/$dir/audeniq-migrate" "target/$dir/audeniq-admin" /out/
FROM debian:13-slim
# ffmpeg/ffprobe are runtime dependencies of the worker: Stage 1 audio QC
# (probe, loudness, clipping, truncation) and fingerprinting shell out to them.
# openssh-client: SFTP partner delivery (DDEX drops) runs `sftp -b -` with a
# pinned known_hosts file (crate::transport::SftpTransport).
# /var/tmp/audeniq is the worker TMPDIR. A new named volume copies this
# ownership, so UID 10001 can write downloaded masters for QC.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates ffmpeg openssh-client libxml2-utils libimage-exiftool-perl && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 audeniq && useradd --uid 10001 --gid 10001 --no-create-home audeniq \
    && install -d -o 10001 -g 10001 -m 700 /var/tmp/audeniq
COPY --from=build /out/ /usr/local/bin/
COPY --from=build /meta/ /usr/local/share/audeniq/
USER 10001:10001
ENV RUST_LOG=info
CMD ["audeniq-api"]
