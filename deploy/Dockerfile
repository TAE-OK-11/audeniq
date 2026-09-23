FROM rust:1.98.1-trixie AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY config ./config
COPY migrations ./migrations
RUN cargo build --locked --release -p audeniq-core --bins
FROM debian:13-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 audeniq && useradd --uid 10001 --gid 10001 --no-create-home audeniq
COPY --from=build /build/target/release/audeniq-api /build/target/release/audeniq-worker /build/target/release/audeniq-migrate /usr/local/bin/
USER 10001:10001
ENV RUST_LOG=info
CMD ["audeniq-api"]
