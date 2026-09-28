-- 0057: DSP expansion (D-12 .. D-29) and Merlin as the default route where
-- Merlin licenses the DSP.
--
-- Added: global/regional streaming (Pandora, SoundCloud, Audiomack,
-- Anghami, Boomplay, JioSaavn, KKBOX, LINE MUSIC, AWA, NetEase Cloud Music,
-- Tencent Music, Napster, iHeartRadio), social / short-form video (Meta,
-- TikTok·CapCut by ByteDance, YouTube Content ID·Shorts, Snapchat) and the
-- Beatport store. The Rust registry (crate::dsp_registry) is the source;
-- dsp_registry::tests::seed_migration_matches_registry pins these rows.
--
-- Same provisioning as 0042: CONTRACTED, delivery_enabled=false,
-- send_or_publish=false, onboarding at INTAKE — nothing can send, and the
-- pre-launch lock (DSP_LIVE_TRANSMISSION) blocks routing on top.
--
-- Contract route: every DSP Merlin licenses starts on the MERLIN route
-- (the Merlin membership agreement covers them); DSPs without a Merlin deal
-- (the Korean services, Qobuz, Beatport) stay DIRECT. Rows an operator
-- already chose (updated_by set) are left alone. Merlin's partner list
-- changes: `audeniq-admin partner merlin-eligible D-n true|false`.
INSERT INTO distribution.dsp_registry(code, dsp_id, slug, name, region, delivery_format) VALUES
 ('D-12','3e7e837f-f363-525b-bc62-42634f0cd55d','pandora','Pandora','GLOBAL','DDEX'),
 ('D-13','3bff2a1a-dd89-54a4-bfee-9da17720ad3a','soundcloud','SoundCloud','GLOBAL','DDEX'),
 ('D-14','8740c5aa-1a0c-574b-b296-c6341eb9a593','audiomack','Audiomack','GLOBAL','DDEX'),
 ('D-15','1c16b974-e0f2-5d04-96db-0da6345175cb','anghami','Anghami','GLOBAL','DDEX'),
 ('D-16','5fd4046f-b356-5132-bb9d-4152288cfa3f','boomplay','Boomplay','GLOBAL','DDEX'),
 ('D-17','5acc0724-270f-57e1-86f0-cfcc71a19836','jiosaavn','JioSaavn','GLOBAL','DDEX'),
 ('D-18','2fdc4bb8-f804-592b-a602-bacfad46ad41','kkbox','KKBOX','GLOBAL','DDEX'),
 ('D-19','60c11645-5562-50ea-845e-cf1a2d57d812','line-music','LINE MUSIC','GLOBAL','DDEX'),
 ('D-20','d4b4930a-feba-5ca6-a19d-d563febd73d6','awa','AWA','GLOBAL','DDEX'),
 ('D-21','ae0621fc-e5a2-5f5e-9aea-4027cdd67cfd','netease','NetEase Cloud Music','GLOBAL','DDEX'),
 ('D-22','143865fc-4902-5f49-ba37-c37585cd1b06','tencent','Tencent Music','GLOBAL','DDEX'),
 ('D-23','092d78f2-b78b-58ef-a265-976ed1595fc0','napster','Napster','GLOBAL','DDEX'),
 ('D-24','05c69127-7a32-50a6-987d-f97dd24b0714','iheart','iHeartRadio','GLOBAL','DDEX'),
 ('D-25','5071facc-a767-561c-b69d-e0535327c05c','meta','Meta','GLOBAL','DDEX'),
 ('D-26','0d197522-8bbd-5e47-97e5-f14e0eae3e5d','tiktok','TikTok','GLOBAL','DDEX'),
 ('D-27','4ea8e91e-82b3-5c81-be86-f3d007571251','youtube-cid','YouTube Content ID','GLOBAL','DDEX'),
 ('D-28','5e1e8abc-ff31-50d2-a674-ce1ae92e294f','snapchat','Snapchat','GLOBAL','DDEX'),
 ('D-29','0753b618-eb03-5dd2-af9a-5ba805027013','beatport','Beatport','GLOBAL','DDEX')
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
WHERE r.code IN ('D-12','D-13','D-14','D-15','D-16','D-17','D-18','D-19','D-20','D-21','D-22','D-23','D-24','D-25','D-26','D-27','D-28','D-29')
ON CONFLICT (partner_id) DO NOTHING;

INSERT INTO execution.partner_onboarding(partner_id)
SELECT code FROM distribution.dsp_registry WHERE code IN ('D-12','D-13','D-14','D-15','D-16','D-17','D-18','D-19','D-20','D-21','D-22','D-23','D-24','D-25','D-26','D-27','D-28','D-29')
ON CONFLICT (partner_id) DO NOTHING;

INSERT INTO distribution.dsp_contract_routes(code, merlin_eligible, route) VALUES
 ('D-12', true, 'MERLIN'),
 ('D-13', true, 'MERLIN'),
 ('D-14', true, 'MERLIN'),
 ('D-15', true, 'MERLIN'),
 ('D-16', true, 'MERLIN'),
 ('D-17', true, 'MERLIN'),
 ('D-18', true, 'MERLIN'),
 ('D-19', true, 'MERLIN'),
 ('D-20', true, 'MERLIN'),
 ('D-21', true, 'MERLIN'),
 ('D-22', true, 'MERLIN'),
 ('D-23', true, 'MERLIN'),
 ('D-24', true, 'MERLIN'),
 ('D-25', true, 'MERLIN'),
 ('D-26', true, 'MERLIN'),
 ('D-27', true, 'MERLIN'),
 ('D-28', true, 'MERLIN'),
 ('D-29', false, 'DIRECT')
ON CONFLICT (code) DO NOTHING;

UPDATE distribution.dsp_contract_routes
   SET route = 'MERLIN', updated_at = now()
 WHERE merlin_eligible AND route = 'DIRECT' AND updated_by IS NULL;
