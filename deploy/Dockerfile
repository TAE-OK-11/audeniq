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
COPY --from=plan /build/recipe.json recipe.json
RUN cargo chef cook --locked --profile "$CARGO_PROFILE" -p audeniq-core --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY config ./config
COPY migrations ./migrations
RUN cargo build --locked --profile "$CARGO_PROFILE" -p audeniq-core --bins \
    && mkdir /out \
    && dir=$([ "$CARGO_PROFILE" = dev ] && echo debug || echo "$CARGO_PROFILE") \
    && cp "target/$dir/audeniq-api" "target/$dir/audeniq-worker" "target/$dir/audeniq-migrate" "target/$dir/audeniq-admin" /out/
FROM debian:13-slim
# ffmpeg/ffprobe are runtime dependencies of the worker: Stage 1 audio QC
# (probe, loudness, clipping, truncation) and fingerprinting shell out to them.
# openssh-client: SFTP partner delivery (DDEX drops) runs `sftp -b -` with a
# pinned known_hosts file (crate::transport::SftpTransport).
# /var/tmp/audeniq is the worker TMPDIR. A new named volume copies this
# ownership, so UID 10001 can write downloaded masters for QC.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates ffmpeg openssh-client libxml2-utils && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 audeniq && useradd --uid 10001 --gid 10001 --no-create-home audeniq \
    && install -d -o 10001 -g 10001 -m 700 /var/tmp/audeniq
COPY --from=build /out/ /usr/local/bin/
USER 10001:10001
ENV RUST_LOG=info
CMD ["audeniq-api"]
