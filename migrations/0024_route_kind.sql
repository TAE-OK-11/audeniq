-- 0024: delivery route taxonomy for multi-path distribution.
--
-- route_kind classifies HOW a partner reaches DSPs:
--   direct     - AUDENIQ delivers DDEX ERN straight to the DSP (e.g. a DSP
--                with a direct ingestion agreement).
--   aggregator - delivery goes to a licensing aggregator that forwards to
--                its member DSPs (e.g. Merlin Network). One delivery job
--                covers every member DSP in the aggregator's footprint.
--   upstream   - delivery goes to an upstream distributor that re-delivers
--                downstream (e.g. LIMBO as upstream). The upstream partner
--                owns the downstream DSP relationships.
--
-- The seeded merlin / limbo-upstream rows are CONTRACTED + delivery_enabled
-- = false: they document the route taxonomy and can never reach the wire
-- until F6 registers a real contract (contract route + ACTIVE endpoint +
-- non-revoked contract revision, per 0022). No synthetic traffic may use
-- these partner_ids.
ALTER TABLE execution.adapter_profiles
  ADD COLUMN route_kind text NOT NULL DEFAULT 'direct'
  CHECK (route_kind IN ('direct','aggregator','upstream'));

COMMENT ON COLUMN execution.adapter_profiles.route_kind IS
  'direct: DDEX ERN straight to the DSP. aggregator: via a licensing aggregator (e.g. Merlin) covering member DSPs. upstream: via an upstream distributor (e.g. LIMBO) that re-delivers downstream.';

INSERT INTO execution.adapter_profiles
  (partner_id, display_name, profile_version, capabilities, delivery_enabled, transport, activation_kind, route_kind)
VALUES
  ('merlin', 'Merlin Network (licensing aggregator)', '1',
   '{"validate_package":true,"prepare_transfer":true,"send_or_publish":false,
     "inquire_submission":false,"parse_ack":false,"get_release_status":false,
     "update_release":false,"takedown":false,"receive_royalty_report":false}',
   false, 'ddex', 'CONTRACTED', 'aggregator'),
  ('limbo-upstream', 'LIMBO (upstream distributor)', '1',
   '{"validate_package":true,"prepare_transfer":true,"send_or_publish":false,
     "inquire_submission":false,"parse_ack":false,"get_release_status":false,
     "update_release":false,"takedown":false,"receive_royalty_report":false}',
   false, 'ddex', 'CONTRACTED', 'upstream')
ON CONFLICT (partner_id) DO NOTHING;
