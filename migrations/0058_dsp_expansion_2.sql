-- 0058: DSP expansion 2 (D-30 .. D-35).
--
-- Checked against the requested platform list:
--   already covered : Spotify, Apple Music, Instagram/Facebook (one Meta
--                     feed), TikTok, YouTube Music, FLO, Amazon, Pandora,
--                     Deezer, TIDAL, iHeartRadio, (Jio)Saavn, Anghami, KKBOX,
--                     Boomplay, Snapchat, NetEase, Tencent Music (QQ Music,
--                     Kugou, Kuwo, WeSing: one TME feed), Audiomack, Qobuz,
--                     YouTube Content ID
--   closed services : Resso (became TikTok Music, closed 2024-11),
--                     Soundtrack by Twitch (closed 2022) — not added
--   added here      : iTunes Store (downloads, separate from Apple Music
--                     streaming), Claro Música, Pretzel, Triller,
--                     TouchTunes, Yandex Music
--
-- Same provisioning as 0042/0057 (CONTRACTED, disabled, INTAKE) and still
-- behind the pre-launch lock. iTunes starts on the MERLIN route like Apple
-- Music; the others have no Merlin deal and are direct-contract targets.
INSERT INTO distribution.dsp_registry(code, dsp_id, slug, name, region, delivery_format) VALUES
 ('D-30','de8a21ee-e802-5ac0-b1d9-aecbd9779ad4','itunes','iTunes Store','GLOBAL','DDEX'),
 ('D-31','631f571e-9e8b-560b-968f-292acdbf6161','claro-musica','Claro Música','GLOBAL','DDEX'),
 ('D-32','64b88716-c27d-541b-b496-71f14d55dabd','pretzel','Pretzel','GLOBAL','DDEX'),
 ('D-33','5f2d7574-eb9e-5c43-a011-0e857013566f','triller','Triller','GLOBAL','DDEX'),
 ('D-34','1e03c91c-e403-5471-9295-65d3672da16d','touchtunes','TouchTunes','GLOBAL','DDEX'),
 ('D-35','67097064-f632-5616-bc5c-28cb9a7edcd5','yandex','Yandex Music','GLOBAL','DDEX')
ON CONFLICT (code) DO NOTHING;

UPDATE distribution.dsp_registry SET name = 'Apple Music' WHERE code = 'D-6';
UPDATE execution.adapter_profiles SET display_name = 'Apple Music' WHERE partner_id = 'D-6';

INSERT INTO execution.adapter_profiles
  (partner_id, display_name, profile_version, dsp_id, capabilities, delivery_enabled, transport, activation_kind, route_kind)
SELECT r.code, r.name, '1', r.dsp_id,
  jsonb_build_object(
    'validate_package', true, 'prepare_transfer', true, 'send_or_publish', false,
    'inquire_submission', false, 'parse_ack', false, 'get_release_status', false,
    'update_release', false, 'takedown', false, 'receive_royalty_report', false),
  false, 'ddex', 'CONTRACTED', 'direct'
FROM distribution.dsp_registry r
WHERE r.code IN ('D-30','D-31','D-32','D-33','D-34','D-35')
ON CONFLICT (partner_id) DO NOTHING;

INSERT INTO execution.partner_onboarding(partner_id)
SELECT code FROM distribution.dsp_registry WHERE code IN ('D-30','D-31','D-32','D-33','D-34','D-35')
ON CONFLICT (partner_id) DO NOTHING;

INSERT INTO distribution.dsp_contract_routes(code, merlin_eligible, route) VALUES
 ('D-30', true, 'MERLIN'),
 ('D-31', false, 'DIRECT'),
 ('D-32', false, 'DIRECT'),
 ('D-33', false, 'DIRECT'),
 ('D-34', false, 'DIRECT'),
 ('D-35', false, 'DIRECT')
ON CONFLICT (code) DO NOTHING;
