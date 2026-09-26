-- 0042: DSP registry (D-1 .. D-11) and pre-provisioned direct routes.
--
-- Every platform Studio offers gets a stable internal code. Call sites use
-- the code (Rust: crate::dsp_registry::Dsp), never the commercial name. The
-- dsp_id is uuid_v5(URL, 'audeniq:dsp:<code>'), the same derivation
-- partner_onboarding::set_dsp uses, and the Rust test
-- dsp_registry::tests::seed_migration_matches_registry pins this seed to
-- the Rust table.
--
-- For each DSP a direct adapter profile is pre-provisioned so onboarding
-- only has to fill in evidence (DPID, endpoint, credential, contract):
-- CONTRACTED, delivery_enabled=false, send_or_publish=false. The 0027 guard
-- still refuses delivery_enabled until partner_onboarding_gaps() is empty,
-- and routing still needs a live contract route. Nothing here can send.
CREATE TABLE distribution.dsp_registry (
 code text PRIMARY KEY CHECK (code ~ '^D-[0-9]{1,3}$'),
 dsp_id uuid NOT NULL UNIQUE,
 slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9-]{2,40}$'),
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
 region text NOT NULL CHECK (region IN ('KR','GLOBAL')),
 delivery_format text NOT NULL CHECK (delivery_format IN ('DDEX','PARTNER_SPEC')),
 status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','SUNSET')),
 created_at timestamptz NOT NULL DEFAULT now()
);
-- Codes are identifiers: once issued they never change meaning.
CREATE TRIGGER immutable_identity BEFORE DELETE ON distribution.dsp_registry
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

INSERT INTO distribution.dsp_registry(code, dsp_id, slug, name, region, delivery_format) VALUES
 ('D-1','6a9ae374-e0f8-551a-874f-52be7a4fa251','melon','Melon','KR','PARTNER_SPEC'),
 ('D-2','af65bc30-3165-555f-8ef4-6e8a11c0b68b','genie','Genie','KR','PARTNER_SPEC'),
 ('D-3','41e7b9c1-dbb4-58dd-85d2-6b927f292f3a','flo','FLO','KR','PARTNER_SPEC'),
 ('D-4','528378c4-e1d2-5416-93f2-721b630a7060','bugs','Bugs','KR','PARTNER_SPEC'),
 ('D-5','299f441c-306f-51eb-a933-571c2e4839bc','spotify','Spotify','GLOBAL','DDEX'),
 ('D-6','898df4b2-f4bc-5969-a4cc-92a4832fa9a7','apple','Apple Music / iTunes','GLOBAL','DDEX'),
 ('D-7','45dcf7a5-8307-5c77-8beb-bef91907684d','youtube','YouTube Music','GLOBAL','DDEX'),
 ('D-8','6bb8c70f-e427-556e-901e-331c8ff87111','amazon','Amazon Music','GLOBAL','DDEX'),
 ('D-9','2a4f05d1-e744-5a63-93ce-a6ff4083b3d3','tidal','TIDAL','GLOBAL','DDEX'),
 ('D-10','17b4e1b3-0418-568f-904d-c8fd0074eec0','deezer','Deezer','GLOBAL','DDEX'),
 ('D-11','6331630f-f9f7-5cea-8223-3c26b31b5140','qobuz','Qobuz','GLOBAL','DDEX');

-- Pre-provisioned direct routes. partner_id = registry code.
INSERT INTO execution.adapter_profiles
  (partner_id, display_name, profile_version, dsp_id, capabilities, delivery_enabled, transport, activation_kind, route_kind)
SELECT r.code, r.name, '1', r.dsp_id,
  jsonb_build_object(
    'validate_package', true, 'prepare_transfer', true, 'send_or_publish', false,
    'inquire_submission', false, 'parse_ack', false, 'get_release_status', false,
    'update_release', false, 'takedown', false, 'receive_royalty_report', false,
    'ddex_preset', CASE WHEN r.code = 'D-6'
      THEN jsonb_build_object('escalate_to_error', jsonb_build_array('DDEX-PREFLIGHT-RELEASE-TYPE'))
      ELSE NULL END),
  false,
  CASE r.delivery_format WHEN 'DDEX' THEN 'ddex' ELSE 'partner' END,
  'CONTRACTED', 'direct'
FROM distribution.dsp_registry r
ON CONFLICT (partner_id) DO NOTHING;

INSERT INTO execution.partner_onboarding(partner_id)
SELECT code FROM distribution.dsp_registry
ON CONFLICT (partner_id) DO NOTHING;
