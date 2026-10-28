# Studio images and cache policy (2026-10-28)

This is an uncommitted implementation proposal. No live AWS, database, server
or CDN configuration has been applied. The serving survey below describes repository
code, not a verified live deployment.

## Serving survey

* `deploy/aws/apps.ts` declares Studio's origin as **backend**, not a public S3
  bucket. `deploy/aws/template.ts` routes its default traffic to the HTTPS API
  origin. Static cache behaviors omit viewer cookies and queries; the default
  forwards viewer state and disables CDN caching. Admin/landing use different
  S3 static origins and are outside this change.
* `crates/core/src/studio_static.rs` serves `STUDIO_DIR`, a local release
  snapshot (normally `/srv/studio/current` -> `releases/<commit>`). It loads
  files and `.br` sidecars in memory and reloads when the symlink target changes.
  It applies `_headers`, redirects and SPA fallback. It does not read uploaded
  images from the release directory.
* `crates/core/src/storage.rs::store_from_env` selects the configured private
  S3-compatible working store using `STORAGE_ENABLED` and existing `S3_*`
  settings. It supports AWS S3, R2, OCI and other compatible endpoints. Uploads
  use quarantine keys, then server-owned registered keys. No public object URL
  is exposed by this implementation.
* `crates/core/src/uploads.rs` admits JPEG/PNG IMAGE uploads. Completion invokes
  `upload_safety::sanitize`, registers the sanitized bytes, size and SHA-256,
  and records `catalog.asset_safety`. `stored_image` checks registration,
  safety/scan status, size and exact content hash before serving. Asset UUIDs
  refer to uploaded files, not to the artist's currently selected photo.
* `crates/core/src/artist_studio.rs` changes `portal.artist_photos.asset_id`;
  earlier uploaded assets remain. Its private artist-photo route and public
  artist-link photo route are mutable aliases. Platform photo requests retain
  their original asset reference (`platform_requests.rs`).
* `catalog.rs` changes a release's `artwork_asset_id`. `smart_links.rs` serves
  the visible release's current cover at a stable slug URL; `staff.rs` and
  `api.rs` also expose authenticated asset image routes.
* The shared `uploads::image_response` previously marked **all** these routes
  private/immutable for one year. A digest ETag alone does not make a mutable
  URL safe for that policy. This change separates aliases and immutable assets.
* Frontend `api/remote.ts::coverImageUrl` uses asset UUIDs for Studio covers.
  Public `SmartLink`, `ArtistPage`, `ArtistDiscography`, `ArtistPress`,
  `ArtistStudio`, `SmartLinkCard`, and `components/PublicPage` use ordinary
  `<img>` URLs or canvas image loads. Artist photos already used `photo_v` or a
  local edit timestamp. `PhotoCropper` produces JPEG in canvas. Inline data/blob
  previews are browser data, not HTTP cache candidates. Private work-note files
  (`work/notes.rs`) request a five-minute private TTL, but the API boundary
  overrides it to no-store; that behavior is unchanged.
* `photo.rs` wraps **an external media engine**, `audeniq-music photo`.
  `deploy/Dockerfile` builds it at `deploy/audeniq-music.rev` into the runtime
  image. The workspace calls metadata/color/QR/OCR/sanitize/PDF operations;
  its pinned CLI source was inspected: `convert` chooses PNG for `.png`
  destinations and JPEG otherwise. It cannot encode AVIF/WebP. The separate
  trusted WebP producer described below does not depend on that engine.

## Cache classes

Origin rules override stale `_headers` instructions. CloudFront's static policy
has MinTTL=0, DefaultTTL=0, MaxTTL=31536000: it honors these origin limits.

| Class | Origin Cache-Control | CDN behavior |
| --- | --- | --- |
| Fingerprinted `/assets/name-HASH.ext` | `public, max-age=31536000, immutable` | Cache, Accept and compression in key |
| Release-owned `/static/*` (not uploaded user media) | `public, max-age=86400, stale-while-revalidate=604800` | Cache |
| Other public images, fonts, audio/video, PDF | `public, max-age=3600, must-revalidate` | Cache matching extensions |
| `/robots.txt` | `public, max-age=300, must-revalidate` | Cache |
| Mutable manifests and other unclassified files | `public, max-age=0, must-revalidate` | No freshness; revalidate |
| HTML and SPA entry points | `no-cache` | No CDN caching by default |
| Discovery `/.well-known/*` | `no-store` | Explicit caching-disabled exclusion |
| `/api` and `/api/*` | Endpoint-specific private policy | Explicit caching-disabled exclusion |
| Public image alias response | `no-store` (307 to revision) | Never cached |
| Private mutable artist/request image alias | `private, no-cache` | Never CDN cached |
| Asset UUID or retained SHA-256 revision image | `private, max-age=31536000, immutable` | Browser only; API stays CDN-disabled |
| Missing preferred derivative at immutable URL | `private, no-cache` (fingerprinted static: `public, max-age=0, must-revalidate`) | Fallback cannot become a year-long cached miss |
| Static redirects, missing assets | `no-store` | No positive freshness |

CloudFront extension patterns match nested paths too. Explicit `/api`, `/api/*`
and `/.well-known/*` behaviors precede them. CORS preflight, API boundary rules,
HTML error codes and `/dashboard/` resolution are unchanged. The template adds
`Accept` to the static cache key and forwards it, alongside normalized gzip/br
keys. `Vary: Accept` alone would not configure a CloudFront cache key.

## Edit freshness and retained URLs

The public photo and cover routes accept an optional `revision=<source-sha256>`
query. A request without it resolves the currently selected safe image, records
its publication in `portal.image_revisions`, and returns a **307, no-store**
redirect to that hash URL. Browsers' existing `<img>` and canvas loads follow
this automatically; JSON API shapes and error codes are unchanged. Unknown or
malformed revision hashes return the existing NOT_FOUND response.

A later edit resolves to a different hash URL. Previously published revisions
still resolve to the old registered asset. Hashes cannot be used to fetch
arbitrary org assets: the publication must match kind, page UUID, org and hash.
Page visibility is checked before both alias and revision reads; normal source
safety/hash checks still run. An old photo can remain available at its published
revision after replacement/removal while the page remains public. Disabling the
page rejects subsequent origin requests. As with existing private immutable
asset caching, previously downloaded browser copies are retained by the client.

Frontend public aliases now use `media=2` to bypass browser entries previously
incorrectly marked immutable, including photo URLs that already had `v=`. This
query is only a migration namespace; the backend computes the real content
identity. Private artist photo screens already change their timestamp query on
edit and each initial load. Uploaded cover asset UUIDs already change on upload.
Mutable aliases never receive immutable headers anymore.

No CloudFront invalidation, AWS identity, distribution ID, wildcard purge or IAM
change is needed: these routes stay outside the CDN cache. No live purge ran.
The exact live prerequisites for a separately approved rollout are migration
`0161_image_revisions.sql`, the new least-privilege grants in `deploy/grants.sql`,
the backend/frontend release together, and application of the reviewed static
cache template. None has been executed. Existing external consumers of old
unversioned URLs can retain their old browser cache; CDN invalidation cannot
clear a browser's immutable entry. First-party updated URLs bypass it.

## AVIF/WebP plumbing and converter proposal

Static JPEG/PNG requests negotiate `original.jpg.avif` then `original.jpg.webp`
sidecars, preserving the source. Only explicit positive `image/avif` or
`image/webp` acceptance opts in; wildcards use the original. `q=0`, invalid
qualities and missing variants fall back safely. ETags derive from the selected
bytes; `Vary` includes Accept and, when applicable, Accept-Encoding. A JPEG ETag
cannot produce 304 for different AVIF bytes. HEAD and conditional GET share the
same representation logic. Sidecars are trusted release artifacts, like other
static files; build them deterministically before publishing a fingerprinted
release, and change its fingerprint if derivative bytes change.

Uploaded images use the same format preference through `image_variants.rs`.
The additive `catalog.image_variants` registry binds a derivative to org, asset
UUID, **sanitized source hash**, type, immutable object key, byte hash and size.
Serving first checks the parent's authorization and safety, then verifies the
selected derivative's recorded size, digest and file signature. A missing,
corrupt or unavailable derivative falls back to the original without exposing
storage keys. Logs contain only asset UUID/format and a failure class. API
responses vary by Accept and remain outside shared CDN caches. A fallback for a
client whose preferred format is not ready requires revalidation so adding a
variant later does not leave that client stuck on a cached fallback.

## Working offline WebP producer

`audeniq-image-encode` now produces real **lossless WebP**, using `image 0.25.10`
with only `jpeg,png,webp` features. Its 12 additional locked packages are Rust
code; no libwebp, dav1d, nasm, FFmpeg, or other native codec toolchain is added.
The image crate's [WebP encoder documentation](https://docs.rs/image/0.25.10/image/codecs/webp/struct.WebPEncoder.html)
explicitly supports lossless encoding only. AVIF via `ravif`/`rav1e` would add a
substantially larger codec dependency graph and needs separate resource and
quality validation. It is not enabled. FFmpeg exists on this development host
but is deliberately absent from the runtime image, so it is not a runtime
reference implementation. The existing engine is also unsuitable: the
[pinned photo CLI](https://github.com/TAE-OK-11/audeniq-media/blob/add30dd14a51c8faba64ddfb5748ab42c4633408/crates/photo-cli/src/lib.rs)
implements PNG/JPEG output only, even in its `convert` command.

The binary is included in the proposed runtime Dockerfile. It is an explicit
operator command, not an HTTP endpoint or automatic upload job:

```sh
# Use the same private working-store settings as the worker, worker-role
# DATABASE_URL, APP_ENV, APP_ORIGIN, EDGE_SERVICE_SECRET, and UPLOAD_AV_ENABLED.
# Run only after separately approved deployment/migration/grants.
audeniq-image-encode ORG_UUID ASSET_UUID SANITIZED_SOURCE_SHA256
# Optional explicit resize and quality mode:
audeniq-image-encode ORG_UUID ASSET_UUID SANITIZED_SOURCE_SHA256 2048 lossless
```

The command accepts a registered asset identity, never a raw upload path. It
loads the source through `stored_image`, requiring the exact registered safety
row, rule version, configured antivirus status, source size and SHA-256. It
checks the operator's expected source hash before creating a child. The child
receives only a private copy of those sanitized bytes and verifies the hash
again. The existing mandatory Linux Landlock/seccomp sandbox clears credentials,
denies networking, exposes the source read-only and a separate output directory
writable, and limits address space to 1 GiB and CPU time to 600 seconds. Failure
to establish the sandbox fails the command; there is no unsandboxed fallback.

Input is limited to 20 MiB, 8192 pixels per edge, 16,777,216 total pixels and a
128 MiB decoder allocation budget. Unsupported/16-bit inputs fail rather than
silently losing precision. Defaults retain dimensions and decoded RGBA pixels;
alpha and any remaining ICC profile are retained. The sanitizer already bakes
orientation and normalizes color to sRGB. `MAX_EDGE` explicitly opts into
Lanczos3 resizing without upscaling. Quality is **lossless**, including by
default; other quality values fail rather than being ignored. Lossless WebP
can exceed a JPEG's byte size; no bandwidth savings are promised.

The output writer refuses bytes beyond the existing **20 MiB hard cap**.
Before leaving the sandbox, the child decodes its output and compares every
RGBA pixel to the intended image. The parent checks size, WebP signature and
SHA-256, uploads under a fresh UUID in `image-variants/`, and reads the object
back for exact byte comparison. It rechecks source safety/hash immediately
before inserting the derivative registry row. `ON CONFLICT DO NOTHING` and the
existing immutable trigger preserve the first published representation, even
if another run chooses different size settings. Repeated runs reuse an existing
size/hash-verified object without re-encoding; a corrupt or unavailable
registered object is an error for the producer and a safe fallback for serving.
The source object and safety row are never updated. A failed storage write/read-back or failed registration
cannot publish a derivative. Crashes/concurrent losing producers may leave
unreferenced objects: cleanup must use a separately reviewed inventory process;
never delete an object after an uncertain commit.

Serving retains the existing parent authorization/safety checks and verifies
derivative hash, size and signature on every read. A missing or corrupt object
falls back to the source. `Accept: image/avif,image/webp` therefore receives real
WebP once registered; **`Accept: image/avif` alone still receives JPEG/PNG**.
The former continues to require revalidation while its preferred AVIF is absent.
Neither the original synthetic AVIF fixture nor its MIME header proves AVIF
decodability. No actual AVIF conversion/serving is claimed.

To finish AVIF, add a bounded sandbox encoder and independent decode/visual
validation, publish through the same source-bound write-once protocol, then run
an end-to-end test that decodes returned AVIF bytes. To make WebP automatic,
add a separately reviewed worker enqueue/retry policy after sanitized upload
registration (or run this explicit command for selected assets). Nothing here
has been deployed, and no automatic background conversion has been enabled.

## Reproducing image tests

`bash deploy/tests/test-image-serving.sh` starts a fresh loopback-only
`postgres:18-trixie` container, runs image unit/static tests, `image_serving`, and the focused
artist-photo execution test changed by PR #353,
and removes the container on exit. Requires Docker and the Rust toolchain, and
Linux sandbox support for the real encoder test. It never reads a live database
URL. With an independently provisioned disposable PostgreSQL instance, run:

```sh
DATABASE_URL=postgres://TEST_OWNER@127.0.0.1:TEST_PORT/postgres \
  cargo test --locked -p audeniq-core --test image_serving -- --nocapture
cargo test --locked -p audeniq-core --lib image_
cargo test --locked -p audeniq-core --lib studio_static
```

The SQLx test owner must have `CREATEDB`; each test creates its own database and
runs the full repository migrations. Fixtures create safety-registered synthetic
assets; the real producer test additionally supplies an actual generated PNG,
executes the sandboxed binary, registers the output, requests HTTP bytes and
decodes WebP for exact RGBA comparison. It also proves AVIF-only fallback,
source hash/safety rejection, write-once publication, and corrupt/missing object
fallback. No frontend code changes are required.
