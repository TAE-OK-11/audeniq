-- F5.5: DDEX ERN 3.8.2 per-DSP messages.
--
-- preparation emits one real ERN 3.8.2 NewReleaseMessage per DSP in the frozen
-- route plan, stored here. The internal synthetic ERN (preparation_artifacts)
-- stays the preflight integrity envelope; this table is the interchange
-- artifact. Wire transmission of these messages is F6 work.
--
-- DPID configuration is partner onboarding data (F6). A DSP without a
-- configured recipient DPID, or an org without a sender DPID, simply gets no
-- row: preparation never invents party identifiers.

ALTER TABLE identity.orgs
  ADD COLUMN IF NOT EXISTS ddex_sender_dpid TEXT NULL
    CHECK (ddex_sender_dpid IS NULL OR length(btrim(ddex_sender_dpid)) BETWEEN 1 AND 64);

ALTER TABLE execution.adapter_profiles
  ADD COLUMN IF NOT EXISTS ddex_recipient_dpid TEXT NULL
    CHECK (ddex_recipient_dpid IS NULL OR length(btrim(ddex_recipient_dpid)) BETWEEN 1 AND 64);

CREATE TABLE IF NOT EXISTS distribution.ddex_messages (
  package_id uuid NOT NULL REFERENCES distribution.distribution_packages(id) ON DELETE CASCADE,
  org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
  dsp_id uuid NOT NULL,
  sender_name text NOT NULL CHECK (length(btrim(sender_name)) BETWEEN 1 AND 200),
  sender_dpid text NOT NULL CHECK (length(btrim(sender_dpid)) BETWEEN 1 AND 64),
  recipient_name text NOT NULL CHECK (length(btrim(recipient_name)) BETWEEN 1 AND 200),
  recipient_dpid text NOT NULL CHECK (length(btrim(recipient_dpid)) BETWEEN 1 AND 64),
  ern_xml text NOT NULL CHECK (length(ern_xml) BETWEEN 1 AND 8000000),
  ern_sha256 text NOT NULL CHECK (ern_sha256 ~ '^[a-f0-9]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (package_id, dsp_id),
  UNIQUE (org_id, package_id, dsp_id)
);
ALTER TABLE distribution.ddex_messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE distribution.ddex_messages FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS ddex_messages_org_scope ON distribution.ddex_messages;
CREATE POLICY ddex_messages_org_scope ON distribution.ddex_messages
  USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid)
  WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid);

-- Test-only DPIDs for the local MockDSP profile. Clearly fake: the TESTDPID
-- prefix can never collide with a real DDEX party identifier.
UPDATE execution.adapter_profiles
  SET ddex_recipient_dpid = 'TESTDPID-MOCKDSP-0001'
  WHERE partner_id = 'mockdsp' AND ddex_recipient_dpid IS NULL;
