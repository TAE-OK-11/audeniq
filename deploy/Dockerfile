FROM rust:1.98.1-trixie AS build
# release(기본): LTO까지 한 운영 빌드. fast: 테스트 서버용으로 빨리 컴파일 (Cargo.toml [profile.fast])
ARG CARGO_PROFILE=release
WORKDIR /build
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
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates ffmpeg && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 audeniq && useradd --uid 10001 --gid 10001 --no-create-home audeniq
COPY --from=build /out/ /usr/local/bin/
USER 10001:10001
ENV RUST_LOG=info
CMD ["audeniq-api"]
