-- 0157: 부가서비스 주문 (crates/core/src/addons.rs).
--
-- Paid add-ons an artist orders on top of distribution, paid either from the
-- settlement balance (a ledger posting, only through finance.charge_addon)
-- or by card (Toss Payments, confirmed server-side):
--  - MV_DISTRIBUTION: a music video to the major video DSPs. Tier BASIC
--    (already rated: distribution only), REVIEW (+ 영상물등급위원회 rating
--    filed for the artist), EXPRESS (+ expedited handling).
--  - EXPRESS_RELEASE: a release reviewed and delivered first so it can go
--    out three business days after submission. Made by submit when the
--    application asks for it (draft options.express).
--  - LYRIC_SYNC: time-synced lyrics (LRC) made for the chosen tracks and
--    registered with lyric services; priced per track.
-- An order is AWAITING_PAYMENT until paid, then SUBMITTED → IN_PROGRESS →
-- DELIVERED → COMPLETED as staff work it; the artist can cancel until work
-- starts, staff can reject. Cancelling or rejecting a paid order refunds it
-- (settlement: an offsetting posting via finance.refund_addon; card: the
-- API cancels the card payment first).
CREATE TABLE portal.addon_orders (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 service text NOT NULL CHECK (service IN ('MV_DISTRIBUTION','EXPRESS_RELEASE','LYRIC_SYNC')),
 tier text NOT NULL DEFAULT '' CHECK (tier IN ('','BASIC','REVIEW','EXPRESS')),
 release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 track_ids uuid[] NOT NULL DEFAULT '{}' CHECK (cardinality(track_ids) <= 100),
 title text NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 200),
 detail jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(detail) = 'object' AND length(detail::text) <= 20000),
 quantity integer NOT NULL CHECK (quantity BETWEEN 1 AND 100),
 unit_price integer NOT NULL CHECK (unit_price BETWEEN 0 AND 10000000),
 amount integer NOT NULL CHECK (amount = unit_price * quantity),
 status text NOT NULL DEFAULT 'AWAITING_PAYMENT'
   CHECK (status IN ('AWAITING_PAYMENT','SUBMITTED','IN_PROGRESS','DELIVERED','COMPLETED','CANCELLED','REJECTED')),
 pay_method text NULL CHECK (pay_method IN ('SETTLEMENT','CARD')),
 paid_at timestamptz NULL,
 ledger_tx uuid NULL,
 pg_order_id text NULL UNIQUE CHECK (pg_order_id ~ '^[A-Za-z0-9_-]{6,64}$'),
 pg_payment_key text NULL CHECK (length(pg_payment_key) <= 200),
 refunded_at timestamptz NULL,
 refund_tx uuid NULL,
 result jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(result) = 'object' AND length(result::text) <= 20000),
 resolution text NOT NULL DEFAULT '' CHECK (length(resolution) <= 1000),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK ((pay_method IS NULL) = (paid_at IS NULL)),
 CHECK (pay_method IS DISTINCT FROM 'SETTLEMENT' OR ledger_tx IS NOT NULL),
 CHECK (pay_method IS DISTINCT FROM 'CARD' OR pg_payment_key IS NOT NULL)
);
CREATE INDEX addon_orders_org ON portal.addon_orders(org_id, created_at DESC);
CREATE INDEX addon_orders_open ON portal.addon_orders(status, created_at)
 WHERE status IN ('SUBMITTED','IN_PROGRESS','DELIVERED');
-- One live express order per release (a resubmission reuses it).
CREATE UNIQUE INDEX addon_orders_express_once ON portal.addon_orders(release_id)
 WHERE service = 'EXPRESS_RELEASE' AND status NOT IN ('CANCELLED','REJECTED');

CREATE TABLE portal.addon_order_events (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 order_id uuid NOT NULL REFERENCES portal.addon_orders ON DELETE CASCADE,
 status text NOT NULL,
 note text NOT NULL DEFAULT '' CHECK (length(note) <= 1000),
 by_staff boolean NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX addon_order_events_order ON portal.addon_order_events(order_id, created_at);

-- Time-synced lyrics of a track: uploaded by the artist (free) or made for a
-- LYRIC_SYNC order (staff deliver it). `registered` lists the lyric services
-- staff have registered it with.
CREATE TABLE portal.track_lyric_sync (
 track_id uuid PRIMARY KEY REFERENCES catalog.tracks ON DELETE RESTRICT,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 lrc text NOT NULL CHECK (length(lrc) BETWEEN 1 AND 60000),
 source text NOT NULL CHECK (source IN ('ARTIST','AI')),
 order_id uuid NULL REFERENCES portal.addon_orders ON DELETE RESTRICT,
 registered text[] NOT NULL DEFAULT '{}' CHECK (cardinality(registered) <= 20),
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX track_lyric_sync_release ON portal.track_lyric_sync(release_id);

-- Pay an order from the settlement balance. The API cannot write the ledger;
-- this is the only path: it takes the same per-org lock as payouts, refuses
-- an amount over the withdrawable balance, posts
--   ROYALTY_PAYABLE debit / ADDON_REVENUE credit
-- and marks the order paid (SUBMITTED) in the same transaction.
-- Returns 'OK', 'NOT_PAYABLE' (not awaiting payment) or 'BALANCE_SHORT'.
CREATE FUNCTION finance.charge_addon(p_order uuid) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE o record; tx uuid;
BEGIN
  SELECT id, org_id, amount, status, pay_method, title INTO o FROM portal.addon_orders WHERE id = p_order FOR UPDATE;
  IF NOT FOUND OR o.status <> 'AWAITING_PAYMENT' OR o.pay_method IS NOT NULL THEN RETURN 'NOT_PAYABLE'; END IF;
  PERFORM pg_advisory_xact_lock(hashtextextended(o.org_id::text, 7));
  IF finance.withdrawable(o.org_id, NULL) < o.amount THEN RETURN 'BALANCE_SHORT'; END IF;
  tx := gen_random_uuid();
  INSERT INTO finance.ledger_transactions(id, org_id, transaction_code, currency, source_ref, description)
    VALUES (tx, o.org_id, 'ADDON_CHARGE', 'KRW', jsonb_build_object('addon_order', o.id), left('부가서비스 · ' || o.title, 300));
  INSERT INTO finance.ledger_entries(id, org_id, transaction_id, account, side, amount, currency) VALUES
    (gen_random_uuid(), o.org_id, tx, 'ROYALTY_PAYABLE', 'DEBIT', o.amount, 'KRW'),
    (gen_random_uuid(), o.org_id, tx, 'ADDON_REVENUE', 'CREDIT', o.amount, 'KRW');
  UPDATE portal.addon_orders SET pay_method = 'SETTLEMENT', paid_at = now(), ledger_tx = tx, status = 'SUBMITTED', updated_at = now()
   WHERE id = p_order;
  INSERT INTO portal.addon_order_events(order_id, status, note, by_staff) VALUES (p_order, 'SUBMITTED', '정산금에서 결제', false);
  RETURN 'OK';
END $$;
REVOKE ALL ON FUNCTION finance.charge_addon(uuid) FROM PUBLIC;

-- Give back a settlement payment once the order is CANCELLED or REJECTED
-- (the caller changes the status first, in the same transaction): an
-- offsetting posting, never an edit. Returns 'OK' or 'SKIP' (not paid from
-- the balance, already refunded, or not closed).
CREATE FUNCTION finance.refund_addon(p_order uuid) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE o record; tx uuid;
BEGIN
  SELECT id, org_id, amount, status, pay_method, ledger_tx, refund_tx, title INTO o
    FROM portal.addon_orders WHERE id = p_order FOR UPDATE;
  IF NOT FOUND OR o.pay_method IS DISTINCT FROM 'SETTLEMENT' OR o.ledger_tx IS NULL OR o.refund_tx IS NOT NULL
     OR o.status NOT IN ('CANCELLED','REJECTED') THEN RETURN 'SKIP'; END IF;
  PERFORM pg_advisory_xact_lock(hashtextextended(o.org_id::text, 7));
  tx := gen_random_uuid();
  INSERT INTO finance.ledger_transactions(id, org_id, transaction_code, currency, source_ref, description, reversal_of)
    VALUES (tx, o.org_id, 'ADDON_REFUND', 'KRW', jsonb_build_object('addon_order', o.id), left('부가서비스 환불 · ' || o.title, 300), o.ledger_tx);
  INSERT INTO finance.ledger_entries(id, org_id, transaction_id, account, side, amount, currency) VALUES
    (gen_random_uuid(), o.org_id, tx, 'ADDON_REVENUE', 'DEBIT', o.amount, 'KRW'),
    (gen_random_uuid(), o.org_id, tx, 'ROYALTY_PAYABLE', 'CREDIT', o.amount, 'KRW');
  UPDATE portal.addon_orders SET refund_tx = tx, refunded_at = now(), updated_at = now() WHERE id = p_order;
  RETURN 'OK';
END $$;
REVOKE ALL ON FUNCTION finance.refund_addon(uuid) FROM PUBLIC;

-- Tell the org at each step.
CREATE FUNCTION portal.addon_order_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE what text; link text;
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 what := CASE NEW.service WHEN 'MV_DISTRIBUTION' THEN '뮤직비디오 배급' WHEN 'EXPRESS_RELEASE' THEN '신속 발매'
                          ELSE 'AI 가사 싱크' END;
 link := '/services?order=' || NEW.id;
 IF NEW.status = 'AWAITING_PAYMENT' AND NEW.service = 'EXPRESS_RELEASE' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', '신속 발매 결제가 필요해요.', '결제하면 담당자가 먼저 검토해 3영업일 안에 발매되도록 진행해요.', link);
 ELSIF NEW.status = 'SUBMITTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 신청이 접수됐어요.', '담당자가 확인하고 진행 상황을 알려 드려요.', link);
 ELSIF NEW.status = 'DELIVERED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 결과를 전달했어요.', left(coalesce(nullif(NEW.resolution, ''), '자세한 내용은 부가서비스에서 확인해 주세요.'), 300), link);
 ELSIF NEW.status = 'COMPLETED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || '가 완료됐어요.', left(coalesce(nullif(NEW.resolution, ''), '자세한 내용은 부가서비스에서 확인해 주세요.'), 300), link);
 ELSIF NEW.status = 'REJECTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 신청을 진행하지 못했어요.',
   left(coalesce(nullif(NEW.resolution, ''), '자세한 내용은 부가서비스에서 확인해 주세요.') ||
        CASE WHEN NEW.paid_at IS NOT NULL THEN ' 결제한 금액은 환불해 드려요.' ELSE '' END, 300), link);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER addon_orders_notice AFTER INSERT OR UPDATE OF status ON portal.addon_orders
 FOR EACH ROW EXECUTE FUNCTION portal.addon_order_notice();
