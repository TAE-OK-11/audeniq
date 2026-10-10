-- 0059: every Merlin-licensed service (D-36 .. D-45) + Merlin for FLO and Yandex.
--
-- Checked against Merlin's published partner list and deal announcements:
--   FLO         : Merlin's first direct deal in Korea (2022-04). FLO becomes
--                 Merlin-eligible and starts on the MERLIN route (delivery
--                 is still FLO's own partner feed).
--   Yandex Music: listed among Merlin's partners; MERLIN route, the
--                 sanctions/remittance review stays a partner warning.
--   added here  : Kuaishou (Kwai, SnackVideo), JOOX, TREBEL, Mixcloud,
--                 Twitch (DJ program), Peloton, Canva, Lickd, Adaptr, STYNGR
--   not added   : Triller (Merlin licence ended over unpaid fees, judgment
--                 2025; stays a DIRECT target with a partner warning),
--                 Supernatural (being re-launched by a new company; the
--                 Merlin deal must be re-confirmed), Merlin's AI training
--                 licences (ElevenLabs, Udio: per-rightsholder opt-in, not a
--                 release delivery), Nina (Merlin Connect API licence).
--
-- Same provisioning as 0042/0057/0058 (CONTRACTED, disabled, INTAKE) and
-- still behind the pre-launch lock.
INSERT INTO distribution.dsp_registry(code, dsp_id, slug, name, region, delivery_format) VALUES
 ('D-36','5b8019d3-ca5a-5a54-834c-1f46164f9873','kuaishou','Kuaishou','GLOBAL','DDEX'),
 ('D-37','3829f885-dcaf-54a6-ad42-47d88d01cfb3','joox','JOOX','GLOBAL','DDEX'),
 ('D-38','560e861a-b450-5418-8e71-e6faa1d2dacf','trebel','TREBEL','GLOBAL','DDEX'),
 ('D-39','fb13d0ea-d147-5c82-abd5-f6db4348bae6','mixcloud','Mixcloud','GLOBAL','DDEX'),
 ('D-40','58676bac-dfa3-5448-98a3-4776a60661f9','twitch','Twitch','GLOBAL','DDEX'),
 ('D-41','9cfa0723-b7fa-5dfa-a759-4221a4c572c5','peloton','Peloton','GLOBAL','DDEX'),
 ('D-42','8adcd690-2016-575f-9eba-7c4b21549ca3','canva','Canva','GLOBAL','DDEX'),
 ('D-43','9dc4adb0-535b-5398-ad8c-46848278fd1d','lickd','Lickd','GLOBAL','DDEX'),
 ('D-44','9f4d7bbc-018c-5739-a7e6-2307e4c65347','adaptr','Adaptr','GLOBAL','DDEX'),
 ('D-45','8e77f249-da9d-57b9-b951-9367d59609ba','styngr','STYNGR','GLOBAL','DDEX')
ON CONFLICT (code) DO NOTHING;

INSERT INTO execution.adapter_profiles
  (partner_id, display_name, profile_version, dsp_id, capabilities, delivery_enabled, transport, activation_kind, route_kind)
SELECT r.code, r.name, '1', r.dsp_id,
  jsonb_build_object(
    'validate_package', true, 'prepare_transfer', true, 'send_or_publish', false,
    'inquire_submission', false, 'parse_ack', false, 'get_release_status', false,
    'update_release', false, 'takedown', false, 'receive_royalty_report', false),
  false, 'ddex', 'CONTRACTED', 'direct'
FROM distribution.dsp_registry r
WHERE r.code IN ('D-36','D-37','D-38','D-39','D-40','D-41','D-42','D-43','D-44','D-45')
ON CONFLICT (partner_id) DO NOTHING;

INSERT INTO execution.partner_onboarding(partner_id)
SELECT code FROM distribution.dsp_registry WHERE code IN ('D-36','D-37','D-38','D-39','D-40','D-41','D-42','D-43','D-44','D-45')
ON CONFLICT (partner_id) DO NOTHING;

INSERT INTO distribution.dsp_contract_routes(code, merlin_eligible, route) VALUES
 ('D-36', true, 'MERLIN'),
 ('D-37', true, 'MERLIN'),
 ('D-38', true, 'MERLIN'),
 ('D-39', true, 'MERLIN'),
 ('D-40', true, 'MERLIN'),
 ('D-41', true, 'MERLIN'),
 ('D-42', true, 'MERLIN'),
 ('D-43', true, 'MERLIN'),
 ('D-44', true, 'MERLIN'),
 ('D-45', true, 'MERLIN')
ON CONFLICT (code) DO NOTHING;

-- FLO and Yandex: Merlin-eligible; move to MERLIN unless staff already chose.
UPDATE distribution.dsp_contract_routes SET merlin_eligible = true
 WHERE code IN ('D-3','D-35');
UPDATE distribution.dsp_contract_routes SET route = 'MERLIN'
 WHERE code IN ('D-3','D-35') AND updated_by IS NULL;
