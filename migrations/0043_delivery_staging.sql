-- 0043: delivery staging, one row per (frozen package, requested DSP).
--
-- Written by the worker's `delivery.stage` job (crate::delivery_staging):
-- per-DSP spec findings, the ERN this DSP would receive (a preview on
-- placeholder party ids until real DPIDs exist), the routing verdict and
-- the readiness class. Staff approve rows (crate::staff); E-0 only enqueues
-- a registry DSP whose row is APPROVED and whose route is live.
CREATE TABLE distribution.delivery_staging (
 package_id uuid NOT NULL REFERENCES distribution.distribution_packages(id) ON DELETE RESTRICT,
 dsp_code text NOT NULL REFERENCES distribution.dsp_registry(code) ON DELETE RESTRICT,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL,
 revision_id uuid NOT NULL,
 dsp_id uuid NOT NULL,
 readiness text NOT NULL CHECK (readiness IN ('CONTENT_BLOCKED','AWAITING_PARTNER','READY')),
 checks jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(checks) = 'array'),
 route_status text NOT NULL CHECK (route_status IN ('ROUTABLE','NO_ROUTE')),
 route_reason text NOT NULL CHECK (length(route_reason) BETWEEN 1 AND 200),
 ern_message_id text NULL,
 ern_sha256 text NULL CHECK (ern_sha256 IS NULL OR ern_sha256 ~ '^[a-f0-9]{64}$'),
 ern_xml text NULL CHECK (ern_xml IS NULL OR length(ern_xml) <= 8000000),
 ern_is_preview boolean NOT NULL DEFAULT true,
 approval text NOT NULL DEFAULT 'PENDING' CHECK (approval IN ('PENDING','APPROVED','HELD')),
 approval_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 approval_note text NOT NULL DEFAULT '' CHECK (length(approval_note) <= 1000),
 approval_at timestamptz NULL,
 staged_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (package_id, dsp_code),
 CHECK (approval <> 'APPROVED' OR (readiness <> 'CONTENT_BLOCKED' AND approval_by IS NOT NULL))
);
CREATE INDEX delivery_staging_release ON distribution.delivery_staging(org_id, release_id, staged_at DESC);
CREATE INDEX delivery_staging_queue ON distribution.delivery_staging(approval, readiness, staged_at);
ALTER TABLE distribution.delivery_staging ENABLE ROW LEVEL SECURITY;
ALTER TABLE distribution.delivery_staging FORCE ROW LEVEL SECURITY;
-- Tenant rows by app.org_id; the staff API sets app.staff='on' after it has
-- verified an active staff role (crate::staff), which spans organisations.
CREATE POLICY delivery_staging_scope ON distribution.delivery_staging
 USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid
        OR current_setting('app.staff',true) = 'on')
 WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid
        OR current_setting('app.staff',true) = 'on');
