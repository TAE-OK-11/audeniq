-- 0123: fixes from review.
--
-- An org-wide contract end applies to every release of the org on its date,
-- whatever release-level end a release also has (an artist's later date or
-- a staff termination is kept as its own row, not overwritten).
CREATE OR REPLACE FUNCTION portal.release_term_ended(p_org uuid, p_release uuid)
RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT EXISTS (
  SELECT 1 FROM portal.release_terms t
   WHERE t.org_id = p_org AND t.release_id = p_release AND t.status <> 'CANCELLED'
     AND t.end_date <= (now() AT TIME ZONE 'Asia/Seoul')::date)
 OR EXISTS (
  SELECT 1 FROM portal.org_contract_terms o
   WHERE o.org_id = p_org AND o.status <> 'CANCELLED'
     AND o.end_date <= (now() AT TIME ZONE 'Asia/Seoul')::date)
$$;

-- Split share percentages read "30%" / "25.5%" in ledger descriptions
-- (to_char's FM format left "30." before).
CREATE OR REPLACE FUNCTION portal.split_pct(bps integer) RETURNS text
LANGUAGE sql IMMUTABLE SET search_path = pg_catalog AS $$
 SELECT rtrim(rtrim(to_char(bps / 100.0, 'FM990.00'), '0'), '.') || '%'
$$;

CREATE OR REPLACE FUNCTION finance.settle_split_share(p_share uuid) RETURNS integer
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE s record; a record; owner_tx uuid; rcpt_tx uuid; n integer := 0; label text;
BEGIN
  SELECT sh.id, sh.org_id, sh.status, sh.recipient_org_id, sh.name, sh.share_bps,
         coalesce(nullif(btrim(r.title), ''), '발매') AS title
    INTO s FROM portal.split_shares sh JOIN catalog.releases r ON r.id = sh.release_id
   WHERE sh.id = p_share FOR UPDATE OF sh;
  IF NOT FOUND OR s.status = 'INVITED' THEN RETURN 0; END IF;
  FOR a IN SELECT * FROM finance.split_allocations WHERE share_id = p_share AND status = 'HELD' ORDER BY created_at FOR UPDATE LOOP
    owner_tx := gen_random_uuid();
    label := CASE WHEN s.status = 'ACCEPTED' THEN s.title || ' 공동작업 분배 전달 (' || s.name || ')'
                  ELSE s.title || ' 공동작업 분배 반환 (' || s.name || ')' END;
    INSERT INTO finance.ledger_transactions(id, org_id, transaction_code, currency, source_ref, description)
      VALUES (owner_tx, a.org_id, CASE WHEN s.status = 'ACCEPTED' THEN 'SPLIT_TRANSFER' ELSE 'SPLIT_RETURN' END,
              a.currency, jsonb_build_object('allocation', a.id, 'share', p_share, 'report', a.report_id), left(label, 300));
    INSERT INTO finance.ledger_entries(id, org_id, transaction_id, account, side, amount, currency, isrc) VALUES
      (gen_random_uuid(), a.org_id, owner_tx, 'SPLIT_HELD', 'DEBIT', a.amount, a.currency, a.isrc),
      (gen_random_uuid(), a.org_id, owner_tx,
       CASE WHEN s.status = 'ACCEPTED' THEN 'SPLIT_TRANSFER_OUT' ELSE 'ROYALTY_PAYABLE' END,
       'CREDIT', a.amount, a.currency, a.isrc);
    IF s.status = 'ACCEPTED' THEN
      rcpt_tx := gen_random_uuid();
      INSERT INTO finance.ledger_transactions(id, org_id, transaction_code, currency, source_ref, description)
        VALUES (rcpt_tx, s.recipient_org_id, 'SPLIT_RECEIVED', a.currency,
                jsonb_build_object('allocation', a.id, 'share', p_share, 'report', a.report_id, 'from_org', a.org_id),
                left(s.title || ' 공동작업 분배 (' || portal.split_pct(s.share_bps) || ')', 300));
      INSERT INTO finance.ledger_entries(id, org_id, transaction_id, account, side, amount, currency, isrc) VALUES
        (gen_random_uuid(), s.recipient_org_id, rcpt_tx, 'SPLIT_TRANSFER_IN', 'DEBIT', a.amount, a.currency, a.isrc),
        (gen_random_uuid(), s.recipient_org_id, rcpt_tx, 'ROYALTY_PAYABLE', 'CREDIT', a.amount, a.currency, a.isrc);
      UPDATE finance.split_allocations SET status = 'TRANSFERRED', recipient_org_id = s.recipient_org_id,
             settled_transaction_id = rcpt_tx, settled_at = now() WHERE id = a.id;
    ELSE
      UPDATE finance.split_allocations SET status = 'RETURNED', settled_transaction_id = owner_tx, settled_at = now()
       WHERE id = a.id;
    END IF;
    n := n + 1;
  END LOOP;
  RETURN n;
END $$;
