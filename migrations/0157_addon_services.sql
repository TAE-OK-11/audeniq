-- 0157: 부가서비스 주문 (crates/core/src/addons.rs).
--
-- Paid add-ons an artist orders on top of distribution, paid either from the
-- settlement balance (a ledger posting, only through finance.charge_addon)
-- or by card (the configured PG, confirmed server-side):
--  - MV_DISTRIBUTION: a music video to the major video DSPs. Tier BASIC
--    (already rated: distribution only), REVIEW (+ 영상물등급위원회 rating
--    filed for the artist), EXPRESS (+ expedited handling).
--  - EXPRESS_RELEASE: a release reviewed and delivered first so it can go
--    out three business days after submission.
--  - LYRIC_SYNC: time-synced lyrics (LRC) made for the chosen tracks and
--    registered with lyric services; priced per track.
-- All three are chosen in the release application (draft options) and made
-- by submit; the artist pays them together at the end of the application or
-- later from the release.
-- An order is AWAITING_PAYMENT until paid, then SUBMITTED → IN_PROGRESS →
-- DELIVERED → COMPLETED as staff work it; the artist can cancel until work
-- starts, staff can reject. Cancelling or rejecting a paid order refunds it
-- (settlement: an offsetting posting via finance.refund_addon; card: the
-- API cancels that order's part of the card payment first).
-- Card payments: one PG payment can pay several orders of a release at once
-- (the checkout at the end of the application). `provider` is the PG it went
-- through (crates/core/src/payments.rs); `pg_key`/`pg_date` are what a later
-- (partial) cancel needs.
CREATE TABLE portal.addon_payments (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 order_ids uuid[] NOT NULL CHECK (cardinality(order_ids) BETWEEN 1 AND 20),
 amount integer NOT NULL CHECK (amount > 0),
 provider text NOT NULL CHECK (provider IN ('toss','portone','payple')),
 pg_order_id text NOT NULL UNIQUE CHECK (pg_order_id ~ '^[A-Za-z0-9_-]{6,64}$'),
 pg_key text NULL CHECK (length(pg_key) <= 200),
 pg_date text NULL CHECK (pg_date ~ '^[0-9]{8}$'),
 status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','PAID','FAILED')),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 paid_at timestamptz NULL
);
CREATE INDEX addon_payments_org ON portal.addon_payments(org_id, created_at DESC);

CREATE TABLE portal.addon_orders (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 service text NOT NULL CHECK (service IN ('MV_DISTRIBUTION','EXPRESS_RELEASE','LYRIC_SYNC')),
 tier text NOT NULL DEFAULT '' CHECK (tier IN ('','BASIC','REVIEW','EXPRESS')),
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
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
 payment_id uuid NULL REFERENCES portal.addon_payments ON DELETE RESTRICT,
 refunded_at timestamptz NULL,
 refund_tx uuid NULL,
 result jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(result) = 'object' AND length(result::text) <= 20000),
 resolution text NOT NULL DEFAULT '' CHECK (length(resolution) <= 1000),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK ((pay_method IS NULL) = (paid_at IS NULL)),
 CHECK (pay_method IS DISTINCT FROM 'SETTLEMENT' OR ledger_tx IS NOT NULL),
 CHECK (pay_method IS DISTINCT FROM 'CARD' OR payment_id IS NOT NULL)
);
CREATE INDEX addon_orders_org ON portal.addon_orders(org_id, created_at DESC);
CREATE INDEX addon_orders_release ON portal.addon_orders(release_id);
CREATE INDEX addon_orders_open ON portal.addon_orders(status, created_at)
 WHERE status IN ('SUBMITTED','IN_PROGRESS','DELIVERED');
-- One live order of each service per release: a resubmission updates the
-- unpaid one (or keeps a paid one) instead of adding another.
CREATE UNIQUE INDEX addon_orders_live_once ON portal.addon_orders(release_id, service)
 WHERE status NOT IN ('CANCELLED','REJECTED');

-- Music video files uploaded in the application (multipart, straight to
-- storage under addon-video/). Never parsed or served to the public: the
-- container is sniffed at completion and staff download it as an attachment.
CREATE TABLE portal.addon_files (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 object_key text NOT NULL UNIQUE CHECK (object_key ~ '^addon-video/'),
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
 size_bytes bigint NOT NULL CHECK (size_bytes > 0),
 content_type text NOT NULL CHECK (content_type IN ('video/mp4','video/quicktime')),
 upload_id text NOT NULL CHECK (length(upload_id) BETWEEN 1 AND 1024),
 status text NOT NULL DEFAULT 'UPLOADING' CHECK (status IN ('UPLOADING','READY','FAILED')),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 completed_at timestamptz NULL
);
CREATE INDEX addon_files_org ON portal.addon_files(org_id, created_at DESC);

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
 link := '/releases/' || NEW.release_id || '?addon=' || NEW.id;
 IF NEW.status = 'SUBMITTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 신청이 접수됐어요.', '담당자가 확인하고 진행 상황을 알려 드려요.', link);
 ELSIF NEW.status = 'DELIVERED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 결과를 전달했어요.', left(coalesce(nullif(NEW.resolution, ''), '자세한 내용은 발매 상세의 부가서비스에서 확인해 주세요.'), 300), link);
 ELSIF NEW.status = 'COMPLETED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || '가 완료됐어요.', left(coalesce(nullif(NEW.resolution, ''), '자세한 내용은 발매 상세의 부가서비스에서 확인해 주세요.'), 300), link);
 ELSIF NEW.status = 'REJECTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 신청을 진행하지 못했어요.',
   left(coalesce(nullif(NEW.resolution, ''), '자세한 내용은 발매 상세의 부가서비스에서 확인해 주세요.') ||
        CASE WHEN NEW.paid_at IS NOT NULL THEN ' 결제한 금액은 환불해 드려요.' ELSE '' END, 300), link);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER addon_orders_notice AFTER INSERT OR UPDATE OF status ON portal.addon_orders
 FOR EACH ROW EXECUTE FUNCTION portal.addon_order_notice();
