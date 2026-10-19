#!/bin/sh
# Apply the same CPU settings to cargo-chef and the final build, including
# native dependencies (AWS-LC, jemalloc, compression codecs).
set -eu

arch=${1:?usage: build-target.sh ARCH CPU COMMAND...}
cpu=${2:-}
shift 2
case "$arch" in
  amd64) export CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu ;;
  arm64) export CARGO_BUILD_TARGET=aarch64-unknown-linux-gnu ;;
  *) echo "unsupported architecture: $arch" >&2; exit 1 ;;
esac
# An explicit Cargo target keeps RUSTFLAGS away from build scripts and proc
# macros: ARM CI hosts can compile Graviton4 code without running SVE2 code.
case "$arch:$cpu" in
  amd64:|amd64:x86-64) cpu=x86-64; native_flags=-march=x86-64 ;;
  amd64:x86-64-v2|amd64:x86-64-v3) native_flags="-march=$cpu" ;;
  arm64:|arm64:generic) cpu=generic; native_flags=-march=armv8-a ;;
  arm64:neoverse-v2) native_flags=-mcpu=neoverse-v2 ;;
  arm64:neoverse-n1) native_flags=-mcpu=neoverse-n1 ;;
  arm64:ampere1a) native_flags=-mcpu=ampere1a ;;
  *) echo "unsupported architecture/CPU: $arch:$cpu" >&2; exit 1 ;;
esac

# jemalloc fixes its page size at build time and refuses to start on a kernel
# with larger pages. ARM servers run 4, 16 or 64 KiB page kernels (Oracle
# Linux UEK uses 64 KiB): build for 64 KiB, which works on all of them.
if [ "$arch" = arm64 ]; then export JEMALLOC_SYS_WITH_LG_PAGE=16; fi

export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C target-cpu=$cpu"
export CFLAGS="${CFLAGS:+$CFLAGS }$native_flags"
export CXXFLAGS="${CXXFLAGS:+$CXXFLAGS }$native_flags"
if [ -n "${AUDENIQ_BUILD_METADATA_DIR:-}" ]; then
  mkdir -p "$AUDENIQ_BUILD_METADATA_DIR"
  printf '%s\n' "$cpu" > "$AUDENIQ_BUILD_METADATA_DIR/target-cpu"
  printf '%s\n' "$arch" > "$AUDENIQ_BUILD_METADATA_DIR/target-arch"
fi
exec "$@"
