-- 0119: revenue splits with collaborators (crates/core/src/splits.rs) and
-- royalty posting (crates/core/src/royalty_posting.rs).
--
-- The applicant sets, per track and while the release is editable, which
-- collaborators get what share of that track's net royalty (after the
-- AUDENIQ fee). Each collaborator is invited by email and accepts into their
-- own AUDENIQ account (org), so their share lands in their own balance and is
-- paid out and taxed under their own payout and tax details. The applicant
-- keeps the rest.
--
-- Posting a fully matched, paid royalty report credits each release's owner
-- and moves accepted shares to the collaborators' orgs. Shares not yet
-- accepted are held in the owner's books (SPLIT_HELD) and move when the
-- collaborator accepts, or go back to the owner when they decline.
CREATE TABLE portal.split_shares (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 track_id uuid NOT NULL REFERENCES catalog.tracks ON DELETE RESTRICT,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
 -- Sealed (payout key ring, field SplitEmail) until the invite is answered.
 email_cipher bytea NULL,
 email_bidx text NOT NULL CHECK (email_bidx ~ '^[0-9a-f]{64}$'),
 share_bps integer NOT NULL CHECK (share_bps BETWEEN 1 AND 10000),
 status text NOT NULL DEFAULT 'INVITED' CHECK (status IN ('INVITED','ACCEPTED','DECLINED','REMOVED')),
 recipient_org_id uuid NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 invite_token_hash text NOT NULL UNIQUE CHECK (invite_token_hash ~ '^[0-9a-f]{64}$'),
 invite_status text NOT NULL DEFAULT 'PENDING' CHECK (invite_status IN ('PENDING','SENT','FAILED','SKIPPED')),
 invite_attempts integer NOT NULL DEFAULT 0,
 invite_next_at timestamptz NOT NULL DEFAULT now(),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 decided_at timestamptz NULL,
 CHECK ((status = 'ACCEPTED') = (recipient_org_id IS NOT NULL)),
 CHECK (status = 'INVITED' OR email_cipher IS NULL),
 CHECK (recipient_org_id IS NULL OR recipient_org_id <> org_id)
);
-- One live share per collaborator and track.
CREATE UNIQUE INDEX split_shares_one_per_person ON portal.split_shares(track_id, email_bidx) WHERE status <> 'REMOVED';
CREATE INDEX split_shares_release ON portal.split_shares(org_id, release_id);
CREATE INDEX split_shares_recipient ON portal.split_shares(recipient_org_id) WHERE recipient_org_id IS NOT NULL;
CREATE INDEX split_shares_invites ON portal.split_shares(invite_next_at) WHERE invite_status = 'PENDING';

-- One posting per royalty report: the FX rate and who posted it.
CREATE TABLE finance.royalty_postings (
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 report_id uuid NOT NULL,
 fx_rate numeric NOT NULL CHECK (fx_rate > 0),
 received_amount numeric NOT NULL CHECK (received_amount > 0),
 gross_krw numeric NOT NULL CHECK (gross_krw >= 0),
 posted_by uuid NOT NULL,
 posted_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (org_id, report_id),
 FOREIGN KEY (org_id, report_id) REFERENCES finance.royalty_reports(org_id, id) ON DELETE RESTRICT
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON finance.royalty_postings
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- Every collaborator amount of every posting: held, moved to the
-- collaborator, or returned to the owner.
CREATE TABLE finance.split_allocations (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 share_id uuid NOT NULL REFERENCES portal.split_shares ON DELETE RESTRICT,
 report_id uuid NOT NULL,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 isrc text NULL,
 amount numeric NOT NULL CHECK (amount > 0),
 currency char(3) NOT NULL DEFAULT 'KRW',
 status text NOT NULL CHECK (status IN ('HELD','TRANSFERRED','RETURNED')),
 recipient_org_id uuid NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 posted_transaction_id uuid NOT NULL,
 settled_transaction_id uuid NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 settled_at timestamptz NULL,
 CHECK ((status = 'HELD') = (settled_transaction_id IS NULL)),
 CHECK ((status = 'TRANSFERRED') = (recipient_org_id IS NOT NULL)),
 FOREIGN KEY (org_id, report_id) REFERENCES finance.royalty_reports(org_id, id) ON DELETE RESTRICT
);
CREATE INDEX split_allocations_share ON finance.split_allocations(share_id, status);
CREATE INDEX split_allocations_recipient ON finance.split_allocations(recipient_org_id) WHERE recipient_org_id IS NOT NULL;

-- Settle what a collaborator's share holds once the invite is answered:
-- accepted → owner SPLIT_HELD → SPLIT_TRANSFER_OUT and, in the collaborator's
-- books, SPLIT_TRANSFER_IN → ROYALTY_PAYABLE; declined or removed → back to
-- the owner's ROYALTY_PAYABLE. The API calls only this (it cannot write the
-- ledger itself); each held amount moves once.
CREATE FUNCTION finance.settle_split_share(p_share uuid) RETURNS integer
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
                left(s.title || ' 공동작업 분배 (' || trim(to_char(s.share_bps / 100.0, 'FM990.99')) || '%)', 300));
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
REVOKE ALL ON FUNCTION finance.settle_split_share(uuid) FROM PUBLIC;

-- Tell both sides when a collaborator answers.
CREATE FUNCTION portal.split_share_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text; tr text; o text;
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status OR NEW.status NOT IN ('ACCEPTED','DECLINED') THEN RETURN NEW; END IF;
 SELECT coalesce(nullif(btrim(title), ''), '발매') INTO t FROM catalog.releases WHERE id = NEW.release_id;
 SELECT title INTO tr FROM catalog.tracks WHERE id = NEW.track_id;
 IF NEW.status = 'ACCEPTED' THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', NEW.name || '님이 ''' || tr || ''' 수익 분배를 수락했어요.',
   '이 곡 정산 수익 중 공동작업 몫은 ' || NEW.name || '님 계정으로 들어가요.', '/releases/' || NEW.release_id);
  SELECT name INTO o FROM identity.orgs WHERE id = NEW.org_id;
  PERFORM portal.notify(NEW.recipient_org_id, 'SETTLEMENT', '''' || tr || ''' 수익 분배를 수락했어요.',
   o || '의 ' || t || ' 수익 중 내 몫이 정산될 때마다 잔액에 쌓여요.', '/settlement');
 ELSE
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', NEW.name || '님이 ''' || tr || ''' 수익 분배를 거절했어요.',
   '그 몫은 신청자 정산으로 돌아와요. 비율을 바꾸려면 발매 상세에서 다시 정해 주세요.', '/releases/' || NEW.release_id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER split_shares_notice AFTER UPDATE OF status ON portal.split_shares
 FOR EACH ROW EXECUTE FUNCTION portal.split_share_notice();
