# Studio images and cache policy (2026-10-28)

This is an uncommitted implementation proposal. No AWS, database, server or CDN
configuration has been applied. The serving survey below describes repository
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
  it contains no AVIF/WebP encoder implementation or established derivative
  upload command. The engine executable is absent in this environment. No
  conversion has been run or claimed.

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

**No converter/producer is implemented or available here.** Consequently no real
uploaded image becomes AVIF/WebP just by applying this patch. To enable it,
propose a separately reviewed media-engine change and pinned engine revision:

1. Add sandboxed AVIF and WebP encoders to the existing photo engine. Establish
   explicit quality, alpha/color/orientation handling, pixel/resource bounds,
   and visual comparisons; do not choose an unmeasured lower-quality default.
2. After the existing sanitized source is accepted, encode derivatives into
   separate files within the parser sandbox; never overwrite the source.
3. Validate output decoding, MIME, limits and SHA-256, then write through the
   existing server-side ObjectStore with unique derivative keys. Register each
   derivative against the sanitized source digest only after its object exists.
   The worker has insert/select permission, not update/delete permission.
4. Publish derivatives before exposing a new source where possible. If a
   converter is absent/fails, retain and serve JPEG/PNG. Do not register fake
   output or claim the original is AVIF by changing its Content-Type.
5. Test real AVIF/WebP decoding and visual quality in the built Docker image,
   then verify CloudFront with different Accept/Accept-Encoding requests after
   separately approved deployment. No new live IAM permission is expected
   beyond the existing working-store object writer.

Synthetic test payloads verify selection, hashes and HTTP/cache behavior. They
are explicitly not proof of real encoding or browser decoding quality. Database
integration tests additionally cover retained old revisions, unknown revisions,
page visibility, parent hash binding and format-specific ETags; their execution
requires a disposable PostgreSQL instance. See the review summary for checks
actually run and their results. Live CDN behavior, database role grants, Docker
media-engine integration and real AVIF/WebP conversion remain unverified here.

## Verification performed in this worktree

* `cargo check -p audeniq-core`: passed; `cargo check -p audeniq-core --test image_serving`: passed (type-check only).
* Focused Rust library tests: 18 passed (13 static-serving, 1 revisions, 2 negotiation/verification, 2 image-cache policy). Test-only CLI overrides disabled dependency optimization and debug info; repository profiles were unchanged.
* `bun run check`: passed. Full Studio frontend suite: 68 files passed, 288 tests passed, 1 skipped.
* AWS template/function suite: 8 passed. `node --check deploy/aws/functions/edge.js`, Rust formatting and `git diff --check`: passed.
* The two PostgreSQL integration tests in `image_serving.rs` were not executed: no disposable PostgreSQL/Docker environment was available. The existing database execution suite was not run.
* No real converter, live CDN request/invalidation, deployment, migration or grant application ran. No shell scripts were changed.
