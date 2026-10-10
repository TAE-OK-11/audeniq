-- Studio portal: artist-facing features that were browser-only (profile,
-- payout account, inquiries, notifications, contract/rights documents,
-- signed release applications). Operational truth stays in catalog/rights/
-- finance; these tables hold portal records and user-visible state only.
CREATE SCHEMA IF NOT EXISTS portal;

-- Proof documents (licences, consent letters) uploaded like other assets.
ALTER TABLE catalog.assets DROP CONSTRAINT IF EXISTS assets_kind_check;
ALTER TABLE catalog.assets ADD CONSTRAINT assets_kind_check CHECK(kind IN ('AUDIO','IMAGE','DOCUMENT'));

CREATE TABLE portal.artist_profiles (
 user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE RESTRICT,
 display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 120),
 contact_email text NOT NULL DEFAULT '' CHECK(length(contact_email) <= 254),
 bio text NOT NULL DEFAULT '' CHECK(length(bio) <= 1500),
 country char(2) NOT NULL DEFAULT 'KR' CHECK(country ~ '^[A-Z]{2}$'),
 row_version bigint NOT NULL DEFAULT 0,
 updated_at timestamptz NOT NULL DEFAULT now()
);

-- One payout account per organisation. The full account number is stored
-- only as AES-256-GCM ciphertext (key outside the database); the API returns
-- bank, holder and last digits.
CREATE TABLE portal.payout_accounts (
 org_id uuid PRIMARY KEY REFERENCES identity.orgs ON DELETE RESTRICT,
 payee_type text NOT NULL CHECK(payee_type IN ('INDIVIDUAL','SOLE_PROPRIETOR','CORPORATION')),
 holder_name text NOT NULL CHECK(length(btrim(holder_name)) BETWEEN 1 AND 120),
 bank_name text NOT NULL CHECK(length(btrim(bank_name)) BETWEEN 1 AND 80),
 account_last4 text NOT NULL CHECK(account_last4 ~ '^[0-9]{2,4}$'),
 account_cipher bytea NOT NULL CHECK(octet_length(account_cipher) BETWEEN 29 AND 128),
 key_version smallint NOT NULL DEFAULT 1,
 registered_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 registered_at timestamptz NOT NULL DEFAULT now(),
 row_version bigint NOT NULL DEFAULT 0
);

CREATE TABLE portal.inquiries (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 category text NOT NULL CHECK(category IN ('RELEASE','SETTLEMENT','CONTRACT','ACCOUNT','OTHER')),
 release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 subject text NOT NULL CHECK(length(btrim(subject)) BETWEEN 1 AND 200),
 status text NOT NULL DEFAULT 'OPEN' CHECK(status IN ('OPEN','ANSWERED','CLOSED')),
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX inquiries_org ON portal.inquiries(org_id, created_at DESC);

CREATE TABLE portal.inquiry_messages (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 inquiry_id uuid NOT NULL REFERENCES portal.inquiries ON DELETE RESTRICT,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 author_kind text NOT NULL CHECK(author_kind IN ('ARTIST','STAFF')),
 author_user uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 body text NOT NULL CHECK(length(btrim(body)) BETWEEN 1 AND 4000),
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX inquiry_messages_thread ON portal.inquiry_messages(inquiry_id, created_at);

-- user_id NULL = every member of the organisation.
CREATE TABLE portal.notifications (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 user_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 kind text NOT NULL CHECK(kind IN ('RELEASE','DOCUMENT','SETTLEMENT','INQUIRY','ACCOUNT','SYSTEM')),
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 200),
 detail text NOT NULL DEFAULT '' CHECK(length(detail) <= 1000),
 link text NOT NULL DEFAULT '' CHECK(link = '' OR link ~ '^/[A-Za-z0-9/_?=&%.-]*$'),
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX notifications_org ON portal.notifications(org_id, created_at DESC);
CREATE TABLE portal.notification_reads (
 notification_id uuid NOT NULL REFERENCES portal.notifications ON DELETE CASCADE,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 read_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(notification_id, user_id)
);

-- Distribution agreements (signed by the artist after staff review) and
-- rights proofs (requested by staff, answered with an uploaded document).
CREATE TABLE portal.documents (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 kind text NOT NULL CHECK(kind IN ('AGREEMENT','RIGHTS_PROOF')),
 title text NOT NULL CHECK(length(btrim(title)) BETWEEN 1 AND 200),
 version text NOT NULL DEFAULT '1.0' CHECK(length(version) <= 20),
 body text NOT NULL DEFAULT '' CHECK(length(body) <= 20000),
 status text NOT NULL DEFAULT 'REVIEW' CHECK(status IN ('AWAITING_DOCUMENTS','REVIEW','PREPARED','APPROVED','NEEDS','SIGNED')),
 review_note text NOT NULL DEFAULT '' CHECK(length(review_note) <= 1000),
 asset_id uuid NULL REFERENCES catalog.assets ON DELETE RESTRICT,
 file_name text NOT NULL DEFAULT '' CHECK(length(file_name) <= 200),
 checked_at timestamptz NULL,
 signer_name text NOT NULL DEFAULT '' CHECK(length(signer_name) <= 120),
 signature text NOT NULL DEFAULT '' CHECK(signature = '' OR (signature LIKE 'data:image/png;base64,%' AND length(signature) <= 60000)),
 signed_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 signed_at timestamptz NULL,
 row_version bigint NOT NULL DEFAULT 0,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK(status <> 'SIGNED' OR (signed_at IS NOT NULL AND signature <> '' AND kind = 'AGREEMENT'))
);
CREATE INDEX documents_org ON portal.documents(org_id, created_at DESC);
CREATE UNIQUE INDEX documents_one_agreement ON portal.documents(org_id, release_id) WHERE kind = 'AGREEMENT';

-- Signed application submitted with a release (studio "배급 신청서").
-- content_hash is the client-side SHA-256 over content + signature; the
-- server keeps it with its own receipt time so either side can re-verify.
CREATE TABLE portal.release_applications (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 application_no text NOT NULL UNIQUE CHECK(application_no ~ '^AUD-[0-9]{8}-[A-Z2-9]{6}$'),
 form text NOT NULL CHECK(length(form) BETWEEN 1 AND 40),
 content_hash text NOT NULL CHECK(content_hash ~ '^[0-9a-f]{64}$'),
 signer_name text NOT NULL CHECK(length(btrim(signer_name)) BETWEEN 1 AND 120),
 signer_role text NOT NULL CHECK(length(signer_role) BETWEEN 1 AND 60),
 agreements text[] NOT NULL CHECK(cardinality(agreements) BETWEEN 1 AND 10),
 signature text NOT NULL CHECK(signature LIKE 'data:image/png;base64,%' AND length(signature) <= 60000),
 client_submitted_at text NOT NULL CHECK(length(client_submitted_at) <= 40),
 submitted_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 received_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX release_applications_release ON portal.release_applications(org_id, release_id, received_at DESC);

-- Payout requests from the portal. The API never writes finance tables:
-- operations turns a REQUESTED row into a finance.payout_orders row
-- (manual approval, BLUEPRINT §8) and links it here.
CREATE TABLE portal.payout_requests (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 requested_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 payee_party_id uuid NOT NULL,
 amount numeric NOT NULL CHECK(amount > 0),
 currency char(3) NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 idempotency_key text NOT NULL CHECK(length(idempotency_key) BETWEEN 8 AND 120),
 status text NOT NULL DEFAULT 'REQUESTED' CHECK(status IN ('REQUESTED','ORDERED','REJECTED','CANCELLED')),
 payout_order_id uuid NULL REFERENCES finance.payout_orders ON DELETE RESTRICT,
 note text NOT NULL DEFAULT '' CHECK(length(note) <= 500),
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id, idempotency_key)
);
CREATE INDEX payout_requests_org ON portal.payout_requests(org_id, created_at DESC);

-- ---------------------------------------------------------------------------
-- Notifications raised by the pipeline itself. Trigger functions run as the
-- migration owner (SECURITY DEFINER) so the worker and API roles need no
-- write grant on portal tables to raise them.
-- ---------------------------------------------------------------------------
CREATE FUNCTION portal.notify(p_org uuid, p_kind text, p_title text, p_detail text, p_link text) RETURNS void
LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog AS $$
 INSERT INTO portal.notifications(org_id, kind, title, detail, link)
 VALUES (p_org, p_kind, left(p_title, 200), left(p_detail, 1000), p_link)
$$;

CREATE FUNCTION portal.release_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text := coalesce(nullif(btrim(NEW.title), ''), '발매');
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.status LIKE '%\_CORRECTION' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '에 보완 요청이 있어요.',
   '발매 관리에서 ‘보완하기’를 누르면 고칠 곳으로 바로 이동해요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'SUBMITTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 신청이 접수됐어요.',
   '담당자 검토가 시작됐어요. 진행 상황은 발매 관리에서 볼 수 있어요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'ON_HOLD_RIGHTS' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '의 권리 확인이 필요해요.',
   '권리·보완 서류에서 요청된 증빙을 제출해 주세요.', '/rights');
 ELSIF NEW.status = 'READY_FOR_DELIVERY' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 검토가 끝났어요.',
   '플랫폼 배급이 준비됐어요. 발매일에 맞춰 공개돼요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'LIVE' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '이(가) 발매됐어요.',
   '플랫폼에 공개됐어요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'TAKEN_DOWN' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '이(가) 플랫폼에서 내려갔어요.',
   '자세한 내용은 문의로 확인해 주세요.', '/releases/' || NEW.id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER releases_status_notice AFTER UPDATE OF status ON catalog.releases
 FOR EACH ROW EXECUTE FUNCTION portal.release_status_notice();

CREATE FUNCTION portal.document_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.kind = 'AGREEMENT' AND NEW.status = 'APPROVED' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '계약서 검토가 끝났어요.',
   left(NEW.title, 150) || ' · 내용을 확인하고 서명해 주세요.', '/contracts');
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'AWAITING_DOCUMENTS' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '제출할 서류가 있어요.', left(NEW.title, 150), '/rights');
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'NEEDS' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '서류 보완 요청이 있어요.',
   left(coalesce(nullif(NEW.review_note, ''), NEW.title), 300), '/rights');
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'APPROVED' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '제출한 서류가 승인됐어요.', left(NEW.title, 150), '/rights');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER documents_status_notice AFTER INSERT OR UPDATE OF status ON portal.documents
 FOR EACH ROW EXECUTE FUNCTION portal.document_status_notice();

CREATE FUNCTION portal.inquiry_reply_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF NEW.author_kind = 'STAFF' THEN
  UPDATE portal.inquiries SET status = 'ANSWERED', updated_at = now() WHERE id = NEW.inquiry_id;
  INSERT INTO portal.notifications(org_id, user_id, kind, title, detail, link)
  SELECT i.org_id, i.created_by, 'INQUIRY', '문의에 답변이 등록됐어요.', left(i.subject, 150), '/inquiries'
  FROM portal.inquiries i WHERE i.id = NEW.inquiry_id;
 ELSE
  UPDATE portal.inquiries SET status = 'OPEN', updated_at = now() WHERE id = NEW.inquiry_id AND status <> 'CLOSED';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER inquiry_messages_notice AFTER INSERT ON portal.inquiry_messages
 FOR EACH ROW EXECUTE FUNCTION portal.inquiry_reply_notice();

CREATE FUNCTION portal.payout_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.status = 'SETTLED' THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', '수익 지급이 완료됐어요.',
   to_char(NEW.amount, 'FM999,999,999,990') || ' ' || NEW.currency || ' · 등록한 계좌로 보냈어요.', '/settlement');
 ELSIF NEW.status IN ('FAILED','RETURNED') THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', '수익 지급이 완료되지 않았어요.',
   '수령 계좌 정보를 확인한 뒤 다시 요청해 주세요.', '/settlement');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER payout_orders_status_notice AFTER UPDATE OF status ON finance.payout_orders
 FOR EACH ROW EXECUTE FUNCTION portal.payout_status_notice();

-- Staff actions are performed by operations tooling (never the browser):
--   SELECT portal.staff_reply(inquiry_id, '답변');
--   UPDATE portal.documents SET status='APPROVED' WHERE id=...;
CREATE FUNCTION portal.staff_reply(p_inquiry uuid, p_body text) RETURNS uuid LANGUAGE sql SET search_path = pg_catalog AS $$
 INSERT INTO portal.inquiry_messages(inquiry_id, org_id, author_kind, body)
 SELECT id, org_id, 'STAFF', p_body FROM portal.inquiries WHERE id = p_inquiry
 RETURNING id
$$;
REVOKE ALL ON FUNCTION portal.notify(uuid, text, text, text, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION portal.staff_reply(uuid, text) FROM PUBLIC;
