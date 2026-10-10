-- 0087: MockDSP as a platform artists can pick in Studio.
--
-- Before the official launch no real DSP is routable (PRE_LAUNCH_LOCKED), so
-- Studio offered nothing to choose and no release could be submitted. This
-- seeds a second sandbox partner, `mockdsp-studio`, that the production
-- Studio lists as "MockDSP (테스트)". It is the same in-process mock as
-- `mockdsp` (no network, nothing leaves the server) with its own fixed DSP id
-- (uuid_v5(URL, 'audeniq:dsp:mockdsp-studio'), the set_dsp scheme).
--
-- Unlike `mockdsp`, Stage 2 adds it to a release's route plan only when the
-- application picked it (crate::mockdsp::STUDIO_PARTNER), so pinning it here
-- never sends other releases to the mock. Studio shows it only while real
-- transmission is locked, or when MOCKDSP_STUDIO=enabled; MOCKDSP_STUDIO=disabled
-- hides it (crate::mockdsp::studio_enabled).
INSERT INTO execution.adapter_profiles
  (partner_id, display_name, profile_version, dsp_id, capabilities, delivery_enabled,
   transport, activation_kind, ddex_recipient_dpid)
VALUES
  ('mockdsp-studio', 'MockDSP (Studio test partner)', '1',
   '357a6db5-5f95-59b9-86a2-c7e90daf3926',
   '{"validate_package":true,"prepare_transfer":true,"send_or_publish":true,
     "inquire_submission":true,"parse_ack":true,"get_release_status":true,
     "update_release":true,"takedown":true,"receive_royalty_report":false}',
   true, 'mock', 'MOCK', 'TESTDPID-MOCKDSP-0002')
ON CONFLICT (partner_id) DO NOTHING;
