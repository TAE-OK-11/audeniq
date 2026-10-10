-- 0022: explicit activation model for delivery partners.
--
-- activation_kind separates test partners from commercial ones:
--   MOCK       - test/synthetic partner (MockDSP). Eligible for delivery via
--                delivery_enabled alone; may use the synthetic preparation
--                envelope on the mock transport.
--   CONTRACTED - commercial partner. delivery_enabled is only an operator
--                kill-switch; eligibility additionally requires the contract
--                route (route enabled + endpoint ACTIVE + non-revoked
--                contract revision). A CONTRACTED profile can never reach the
--                wire on delivery_enabled alone.
--
-- This closes the bypass where any adapter profile with delivery_enabled=true
-- joined the Stage 2 eligible DSP set without a contract.
ALTER TABLE execution.adapter_profiles
  ADD COLUMN activation_kind text NOT NULL DEFAULT 'MOCK'
  CHECK (activation_kind IN ('MOCK','CONTRACTED'));

COMMENT ON COLUMN execution.adapter_profiles.activation_kind IS
  'MOCK: test partner, eligible via delivery_enabled alone. CONTRACTED: commercial partner, delivery_enabled is only a kill-switch; contract route required.';
