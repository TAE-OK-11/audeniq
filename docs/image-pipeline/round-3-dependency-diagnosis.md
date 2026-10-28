# Round 3: encoder dependency diagnosis (2026-10-28)

> Historical checkpoint. See [Phase 2](phase-2.md) for the restored offline cache,
> implemented codecs, measurements and current validation status.

Phase 1 result: no candidate lossy WebP or AVIF encoder is obtainable by Cargo
in this environment. Phase 2 is deliberately not started. Existing round-2
changes, manifests, lockfile, encoding behavior and storage policy are unchanged.

## Reproduction

Cargo: `cargo 1.98.1 (797e8a9bc 2026-08-05)`.
Branch: `codex-audit-fixes-20261028`.

Separate minimal packages were created under the ignored checkout-local
`target/encoder-dependency-probe/{webp,webp-rust,ravif}`. Each has its own
`[workspace]`, empty `src/lib.rs`, and exactly one pinned dependency with
`default-features = false`:

| Package | Exact requirement | Online fetch exit | Offline fetch exit |
|---|---|---:|---:|
| webp | =0.3.1 | 101 | 101 |
| webp-rust | =0.3.2 | 101 | 101 |
| ravif | =0.13.0 | 101 | 101 |

Online invocation, substituting the candidate name:

```sh
CARGO_HOME="$PWD/target/encoder-dependency-probe/cargo-webp" \
  CARGO_NET_RETRY=0 CARGO_HTTP_TIMEOUT=15 \
  cargo fetch --manifest-path target/encoder-dependency-probe/webp/Cargo.toml
```

Separate Cargo homes keep diagnostic writes inside this checkout and avoid
modifying the existing shared cache. Offline probes use a checkout-local copy
of the existing registry index. The original source and archive cache were
also inspected read-only.

Verbatim online error for webp:

```text
    Updating crates.io index
error: failed to get `webp` as a dependency of package `probe-webp v0.0.0 (/home/hatch/workspace/codex-audit-fixes/target/encoder-dependency-probe/webp)`

Caused by:
  failed to load source for dependency `webp`

Caused by:
  unable to update registry `crates-io`

Caused by:
  download of config.json failed

Caused by:
  [7] Could not connect to server (Failed to connect to index.crates.io:443 over proxy hatch-egress-proxy after 2 ms: Could not connect to server)
```

The webp-rust error has the same chain, naming `webp-rust` / `probe-webp-rust`
and its corresponding path, with the identical final 2 ms error. The ravif
error names `ravif` / `probe-ravif` and its corresponding path; its final line:

```text
  [7] Could not connect to server (Failed to connect to index.crates.io:443 over proxy hatch-egress-proxy after 8 ms: Could not connect to server)
```

These are registry bootstrap failures, not evidence of nonexistent versions,
crate compilation failures, or failures in a specific transitive dependency.
Cargo never reached archive download or dependency resolution.

Offline errors, verbatim first lines:

```text
error: no matching package named `webp` found
error: no matching package named `webp-rust` found
error: no matching package named `ravif` found
```

Each reports `location searched: crates.io index` and its isolated probe package.

Direct reachability checks used curl with a 5-second connect timeout,
10-second total timeout, and response bodies discarded. All exited 7:

| URL | Verbatim stderr |
|---|---|
| https://index.crates.io/config.json | `curl: (7) Failed to connect to hatch-egress-proxy port 3128 after 0 ms: Couldn't connect to server` |
| https://static.crates.io/crates/webp/webp-0.3.1.crate | `curl: (7) Failed to connect to hatch-egress-proxy port 3128 after 3 ms: Couldn't connect to server` |
| https://static.crates.io/crates/webp-rust/webp-rust-0.3.2.crate | `curl: (7) Failed to connect to hatch-egress-proxy port 3128 after 0 ms: Couldn't connect to server` |
| https://static.crates.io/crates/ravif/ravif-0.13.0.crate | `curl: (7) Failed to connect to hatch-egress-proxy port 3128 after 0 ms: Couldn't connect to server` |

Trying the index with `curl --noproxy '*'` exited 6:

```text
curl: (6) Could not resolve host: index.crates.io
```

The separate web research tool could retrieve some upstream documentation.
That does not supply crate archives or transitive dependencies to this build.

## Candidate assessment

| Candidate | Implementation / requirements | License | Obtainable here? |
|---|---|---|---|
| webp 0.3.1 | Rust wrapper over native C libwebp through libwebp-sys ^0.9.3; no codec executable needed. libwebp-sys builds bundled C using cc. | Wrapper MIT OR Apache-2.0; libwebp BSD-3-Clause. | No: index and archive requests fail; sources absent. |
| webp-rust 0.3.2 | Upstream describes pure Rust lossy VP8 and lossless VP8L encoding; no C library or external encoder required. Quality/performance untested here. | Upstream project manifest declares MIT; the exact 0.3.2 LICENSE could not be retrieved and must be checked before adoption. | No: index and archive requests fail; sources absent. |
| ravif 0.13.0 / rav1e 0.8.1 | Pure Rust AVIF encoding with default features disabled; omit optional assembly and threading. An independent AVIF decoder is additionally needed for verification. No external encoder required. | ravif BSD-3-Clause; rav1e BSD-2-Clause. | No: index and ravif archive requests fail; neither codec is cached. |
| image 0.25.10 / image-webp 0.2.4 | Already available pure Rust JPEG/PNG decode and lossless WebP encode. image's optional AVIF encoder delegates to unavailable ravif ^0.13. | MIT OR Apache-2.0. | Available, but does not supply lossy WebP or an enabled AVIF encoder. |

Native environment: `/usr/bin/cc`, `/usr/bin/ar` and `/usr/bin/pkg-config`
exist. libwebp runtime shared objects (`libwebp.so.7.1.8`, demux and mux) exist,
but `/usr/include/webp` and the development pkg-config entry are absent.
`pkg-config --modversion libwebp` exits 1:

```text
Package libwebp was not found in the pkg-config search path.
Perhaps you should add the directory containing `libwebp.pc'
to the PKG_CONFIG_PATH environment variable
Package 'libwebp', required by 'virtual:world', not found
```

Therefore a system-development-library integration is dropped. A bundled-C
libwebp-sys build is a potential option because the C toolchain exists, but
cannot be validated without the missing sources. The runtime library alone
does not make the proposed Rust dependencies available. No external codec
binary, ffmpeg, package installation or native-library modification was used.

The existing registry source/archive cache contains image 0.25.10 and
image-webp 0.2.4 (also an older image 0.24.9 source tree), but no webp,
webp-rust, libwebp-sys, ravif, rav1e or native AVIF codec sources. Cached image
AVIF wrapper modules do not include the codec dependency implementations.
Cargo.lock contains image 0.25.10 and image-webp 0.2.4; none of the proposed
lossy/AVIF dependencies is locked.

Upstream evidence:

- [webp 0.3.1 manifest](https://docs.rs/crate/webp/0.3.1/source/Cargo.toml.orig)
- [libwebp-sys bundled C build requirements](https://docs.rs/crate/libwebp-sys/0.9.6)
- [libwebp license](https://github.com/webmproject/libwebp/blob/main/COPYING)
- [webp-rust 0.3.2 codec capabilities](https://docs.rs/crate/webp-rust/0.3.2)
- [webp-rust project manifest; retrieved page identifies version 0.3.0](https://github.com/mith-mmk/webp-rust/blob/main/Cargo.toml)
- [ravif 0.13.0 manifest and features](https://docs.rs/crate/ravif/0.13.0/source/Cargo.toml.orig)
- [rav1e 0.8.1 license](https://docs.rs/crate/rav1e/0.8.1/source/LICENSE)

## Required unblock and remaining work

Provide a working crates.io proxy/DNS path, a reachable registry mirror with
index and archive access, or checksum-verifiable vendored/pre-downloaded sources
for the selected encoders **and their full transitive dependencies**. AVIF
roundtrip verification also needs an obtainable decoder. For webp-rust, verify
the exact release's license before selection. Bundled libwebp needs its C
sources; system linking would instead need development headers/library metadata.

No quality factor, AVIF speed preset, photo/cover measurement table, visual
review, encode-time skip rule or revised storage threshold is claimed. The
existing 10% plus 1 KiB policy remains unvalidated for lossy output. Those are
Phase 2 work and require functioning codecs and representative real samples.
No application tests were run because no application code changed. The six
Cargo fetch probes and five HTTP reachability probes above all failed as
documented. No production, AWS, live-server or deploy/aws action was performed.

## Phase 2 continuation

The historical status above is superseded by [Phase 2](phase-2.md), using the
restored checkout-local offline registry cache. It records implemented codecs,
measurements, verification limitations, policy and current validation.
