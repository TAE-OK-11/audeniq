-- 0090: 분쟁·중단으로 보류한 곡의 정산금은 출금 가능 잔액에서 뺀다.
--
-- finance.finance_holds (0014) can hold an ISRC, a RELEASE or a PARTY, but
-- the withdrawable balance (portal balances: ROYALTY_PAYABLE − pending
-- payouts) only checked PARTY holds, and only as an all-or-nothing block.
-- Money earned by a disputed track stayed withdrawable. This function sums
-- the ROYALTY_PAYABLE lines covered by an active hold so callers subtract it:
--   ISRC    → ledger lines with that ISRC
--   RELEASE → ledger lines whose ISRC belongs to the release (the track's
--             own ISRC or one issued at Stage 3, identifier_assignments)
--   PARTY   → ledger lines credited to that party
-- A line covered by several holds counts once. The result is never negative.
--
-- SECURITY DEFINER: the API role has no grant on identifier_assignments
-- (FORCE RLS on app.org_id). The function scopes every read to p_org and
-- restores the caller's app.org_id before returning.
CREATE FUNCTION finance.held_payable(p_org uuid)
RETURNS numeric
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE
 v_prev text := current_setting('app.org_id', true);
 v_held numeric;
BEGIN
 PERFORM set_config('app.org_id', p_org::text, true);
 SELECT GREATEST(COALESCE(SUM(CASE WHEN e.side = 'CREDIT' THEN e.amount ELSE -e.amount END), 0), 0)
   INTO v_held
   FROM finance.ledger_entries e
  WHERE e.org_id = p_org AND e.account = 'ROYALTY_PAYABLE' AND e.currency = 'KRW'
    AND EXISTS (
     SELECT 1 FROM finance.finance_holds h
      WHERE h.org_id = e.org_id AND h.active AND (
        (h.scope_type = 'ISRC' AND e.isrc IS NOT NULL AND upper(btrim(h.scope_value)) = e.isrc)
        OR (h.scope_type = 'PARTY' AND e.party_id IS NOT NULL AND btrim(h.scope_value) = e.party_id::text)
        OR (h.scope_type = 'RELEASE' AND e.isrc IS NOT NULL AND (
             EXISTS (SELECT 1 FROM catalog.tracks t
                      WHERE t.org_id = e.org_id AND t.isrc = e.isrc AND t.release_id::text = btrim(h.scope_value))
             OR EXISTS (SELECT 1 FROM distribution.identifier_assignments ia
                         WHERE ia.org_id = e.org_id AND ia.kind = 'ISRC' AND ia.identifier = e.isrc
                           AND ia.release_id::text = btrim(h.scope_value))))));
 PERFORM set_config('app.org_id', coalesce(v_prev, ''), true);
 RETURN v_held;
END $$;
REVOKE ALL ON FUNCTION finance.held_payable(uuid) FROM PUBLIC;
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_api') THEN
  GRANT EXECUTE ON FUNCTION finance.held_payable(uuid) TO audeniq_api;
 END IF;
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
  GRANT EXECUTE ON FUNCTION finance.held_payable(uuid) TO audeniq_worker;
 END IF;
END $$;
