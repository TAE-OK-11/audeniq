# Prepared AUDENIQ error pages — not deployed

Design survey (2026-10-28): Studio `src/styles/tokens.css` supplies Audeniq Sans,
Korean system fallbacks, brand blue #3B63F3, pale blue #F5F8FF, ink #171B29,
28px card corners, and 14px button corners. `src/styles/enhance.css:985–1011`
and `src/components/ErrorScreen.tsx` supply the centred white page, blue card,
white circular outline-icon badge, status chip, responsive type, and actions.
Marketing `web/landing/index.html:59–69` and `content/guide.css:12` share the
font, palette, rounded soft surfaces and blue calls to action.

Existing similar surfaces: Studio NotFound/ErrorBoundary, SystemStatus update,
maintenance, server and device dialogs; ContentAdmin maintenance panels;
backend snapshot-missing HTML; workspace 403/404/500/503/template HTML; v5/v6
viewer-request 403/404 bodies; legacy viewer-response maintenance Function.
The legacy viewer-response Function remains unchanged: it cannot handle origin
400+ errors and is not a substitute for independent error-page hosting.

These standalone pages reuse ErrorScreen's structure and exact icon paths.
They preserve existing Korean copy, status labels and link destinations.
No external images, fonts, scripts or stylesheets are needed: a text wordmark
and the brand font's existing Korean system fallbacks work during origin failure.
ErrorScreen itself needs no restyling. The backend embeds
`crates/core/src/studio_snapshot_missing.html` within the crate so Docker's
existing `COPY crates` build remains sufficient; its 60-second refresh remains.

Prepared `../studio-edge-function-v7.js` is based on workspace v6, with only
HTML design changes and removal of historical comments for size. v5 and v6
are untouched. No routing, status, header or JSON-body changes are made there.
Run `node deploy/cloudfront/verify-edge-v7.cjs /path/to/cloudfront-error-pages`
from the checkout to compare v7 with both historical versions and check copies.

Infrastructure proposals (not applied): disable edge caching for health and
readiness; verify error minimum TTLs and existing cached-object invalidation;
host outage pages on an independent origin; configure errors without replacing
API/discovery JSON; audit origin deadlines against accepted mutations and
idempotency; add origin/DB and certificate monitoring; assess static hosting
separation and WAF false positives before changing rules. Origin availability,
DNS, server deployment and old Worker/tunnel cleanup require separate operations.
Code `no-store` cannot evict existing edge objects or override a positive minimum TTL.
