# DSP registry, delivery staging and staff review

Added 2026-09-26 (migrations 0042–0044). Goal: every release is prepared up
to the moment before transmission, per platform, with a staff approval gate,
while real sends stay impossible until a partner is contracted and onboarded.

## 1. DSP registry (`D-1` … `D-11`)

Internal code, never the platform name, is the key everywhere
(`crate::dsp_registry::Dsp`, `distribution.dsp_registry`). The code is also
the direct route's `partner_id`, and `dsp_id = uuid_v5(URL, "audeniq:dsp:D-n")`.
Studio keeps its platform keys (`profile.platforms`); the backend maps them.

| Code | Studio key | Platform | Delivery | Cover (min / max) | Lead days | Credits required |
|---|---|---|---|---|---|---|
| D-1 | melon | Melon | partner spec | 3000 / – | 14 | composer + lyricist |
| D-2 | genie | Genie | partner spec | 3000 / – | 14 | composer + lyricist |
| D-3 | flo | FLO | partner spec | 3000 / – | 14 | composer + lyricist |
| D-4 | bugs | Bugs | partner spec | 3000 / – | 14 | composer + lyricist |
| D-5 | spotify | Spotify | DDEX ERN 3.8.2 | 3000 / 10000 | 7 | – |
| D-6 | apple | Apple Music / iTunes | DDEX ERN 3.8.2 | 3000 / – | 10 | composer (release-type mismatch is an error) |
| D-7 | youtube | YouTube Music | DDEX ERN 3.8.2 | 3000 / – | 7 | – |
| D-8 | amazon | Amazon Music | DDEX ERN 3.8.2 | 3000 / – | 7 | – |
| D-9 | tidal | TIDAL | DDEX ERN 3.8.2 | 3000 / – | 7 | – |
| D-10 | deezer | Deezer | DDEX ERN 3.8.2 | 3000 / 4096 | 14 | composer + lyricist |
| D-11 | qobuz | Qobuz | DDEX ERN 3.8.2 | 3000 / – | 14 | – |

All: square cover, lossless WAV/FLAC, ≥ 44.1 kHz / 16 bit, genre. Korean
services also get the youth-harmful marking note for explicit releases.
Sources and confidence levels: `DSP_CONDITIONS_RESEARCH_2026-09-25.md`. The
Korean services publish no distributor spec, so their format is marked
`PARTNER_SPEC` until a contract supplies it; the values above are the common
baseline, to be replaced by partner documentation.

Each code has a pre-provisioned direct adapter profile: `CONTRACTED`,
`delivery_enabled=false`, `send_or_publish=false`, plus an
`execution.partner_onboarding` row at `INTAKE`. Nothing is sendable until
onboarding evidence (DPID, endpoint, credential, test ERN, test ACK, signed
contract) clears the 0027 guard and a contract route exists. No DPID is
invented: recipient DPIDs are registered during onboarding.

`audeniq-admin dsp list` shows each code's route state and onboarding gaps.

## 2. Pipeline

```
submit ─ Stage 1 ─ Stage 2 ──(REVIEW)── staff decision ── Stage 2 re-run
                     │                                          │
                     └──────────────(PASS)──────────────────────┘
                                         │
                                Stage 3 prepare_release  (READY_FOR_DELIVERY)
                                         │
                         delivery.stage (distribution queue)
                     one delivery_staging row per requested DSP
                                         │
                     staff delivery decision (APPROVE / HOLD)
                                         │
                   E-0 delivery.enqueue: approved AND route live → job
```

- **Scope follows the artist's choice.** Stage 2 only approves registry DSPs
  the application asked for (`release.draft.platforms`, frozen in the
  revision). Legacy drafts without `platforms` ask for every DSP; test
  partners outside the registry are unaffected.
- **Stage 3 uses the frozen revision** (what Stage 1/2 reviewed) for release
  metadata, not the live draft row.
- **Staging** (`crate::delivery_staging`) evaluates each requested DSP:
  - spec checks (table above) → CONTENT findings;
  - DDEX DSPs: the ERN 3.8.2 message this DSP would get, run through
    preflight (with the DSP's escalations), XSD and business rules. With
    real sender/recipient DPIDs it is also written to
    `distribution.ddex_messages` (the wire artifact); otherwise it is a
    preview on `PREVIEW-…` party ids that never leaves the database;
  - partner findings: not in the Stage 2 scope, route not live, DPIDs
    missing, virtual UPC/ISRC, Korean feed spec pending.
  - `readiness` = `CONTENT_BLOCKED` | `AWAITING_PARTNER` | `READY`.
- **Approval** binds to the exact ERN hash: a re-stage that changes the
  message bytes resets the row to `PENDING`.
- **E-0** skips any registry DSP without an `APPROVED`, non-content-blocked
  staging row even when its route is live (test
  `registry_dsp_waits_for_staff_approval_before_send`).

## 3. Staff review

Staff (`identity.staff_members`, roles ADMIN / REVIEWER / OPERATOR /
SUPPORT) work through `/api/staff/*` (docs/API.md). Decisions reuse the
append-only override model Stage 2 already honours, so the pipeline moves
releases; staff never edit check results.

| Decision | Effect | People |
|---|---|---|
| APPROVE | PASS overrides for the open checks → Stage 2 re-run | 1 reviewer; **2** when an open check is `S2_RIGHTS_SCOPE`, `S2_DOCS_ORIGIN`, `S2_SPECIAL_FLAGS` or BLOCKED |
| REQUEST_CORRECTION | open checks → CORRECTION_REQUIRED → `STAGE2_CORRECTION`, notes shown to the artist | 1 |
| REJECT | `STAGE2_REVIEW → WITHDRAWN` (new state edge), artist notified | 1 |

Review checklist a reviewer sees per release (review sheet): Stage 1 audio /
artwork / metadata results, fingerprint and duplicate holds, protected
artist names, declarations (cover, samples, AI, explicit), rights scope,
signed application, documents, and each platform's staging verdict.

## 4. Data path changes

- JSON package/snapshot hashes stream straight into SHA-256
  (`domain::sha256_json`); no serialized copy is built on freeze, Stage 2
  pinning or every delivery attempt.
- Routing decides all DSPs of a package with two queries instead of
  3 queries per DSP plus one per contracted candidate.
- Delivery materialize reads package, snapshot and preparation artifact in
  one round trip (was three) and borrows the route plan instead of cloning it.
- Stage 2 eligibility folds the per-route contract lookup into its route query.
- ERN generation for staging runs on the blocking pool (`xmllint` is a child
  process), off the async workers.

## 5. Still external (not done here)

Real DPIDs, contracts, endpoints, credentials and live transmission; the
Korean services' feed formats; official DDEX certification. The staff web
portal UI itself is not part of this change: the API above is what it calls.
