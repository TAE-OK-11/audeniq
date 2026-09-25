-- 0025: track version field (DDEX VersionTitle) + persisted route decisions.
--
-- tracks.version: version/designation of the recording ("Radio Edit",
-- "2024 Remaster"). Spotify Metadata Style Guide 8.2/8.4 requires version
-- info to live in a dedicated field, not the title.
ALTER TABLE catalog.tracks
    ADD COLUMN version TEXT NOT NULL DEFAULT ''
    CHECK (length(version) <= 200);

-- execution.route_coverage: which DSPs an aggregator/upstream partner claims
-- to reach (e.g. Merlin's member DSP set). Empty until F6 contract
-- onboarding fills it; the routing engine never assumes coverage.
CREATE TABLE execution.route_coverage (
    partner_id text NOT NULL REFERENCES execution.adapter_profiles(partner_id) ON DELETE CASCADE,
    dsp_id uuid NOT NULL,
    PRIMARY KEY (partner_id, dsp_id)
);
-- execution.route_decisions: the routing engine's per-DSP verdict for one
-- frozen package. ROUTABLE = a sendable adapter profile exists on the chosen
-- route_kind (direct preferred, then aggregator/Merlin, then upstream/LIMBO).
-- NO_ROUTE = nothing sendable; the package waits for F6 contracts instead
-- of failing. Never a transmission trigger by itself.
CREATE TABLE execution.route_decisions (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL,
    package_id uuid NOT NULL,
    dsp_id uuid NOT NULL,
    -- NULL when status = NO_ROUTE (no route kind was chosen).
    route_kind text CHECK (route_kind IN ('direct', 'aggregator', 'upstream')),
    partner_id text,
    status text NOT NULL CHECK (status IN ('ROUTABLE', 'NO_ROUTE')),
    reason text NOT NULL CHECK (length(reason) BETWEEN 1 AND 200),
    decided_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, package_id, dsp_id)
);
