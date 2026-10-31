-- 0168: publishing contracts (저작권 관리 계약서), as distribution has them.
--
-- The application (신청서, 0167) is where the artist picks the works and the
-- conditions and signs the application. Staff review it; approving it now makes
-- the contract (계약서) with the confirmed terms. The artist reads it, ticks the
-- confirmations and signs; only then is the contract in force and the works go
-- on to society registration (REGISTER needs an active contract).
--
-- The body is rendered by the server when the contract is made and kept as it
-- was; the signature, the ticks and a hash over body + terms + ticks + signature
-- are kept with it, like portal.documents agreements.
CREATE TABLE portal.pub_contracts (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 application_id uuid NOT NULL UNIQUE,
 contract_no text NOT NULL UNIQUE CHECK (contract_no ~ '^AQP-[0-9]{8}-[A-Z0-9]{6}$'),
 status text NOT NULL DEFAULT 'pending_sign' CHECK (status IN ('pending_sign','active','expiring','terminating','ended')),
 commission_bps integer NOT NULL CHECK (commission_bps BETWEEN 0 AND 10000),
 term_months integer NOT NULL CHECK (term_months BETWEEN 1 AND 120),
 territories text[] NOT NULL CHECK (cardinality(territories) BETWEEN 1 AND 10),
 rights text[] NOT NULL CHECK (cardinality(rights) BETWEEN 1 AND 10),
 body text NOT NULL CHECK (length(body) BETWEEN 1 AND 20000),
 signer_name text NULL CHECK (length(signer_name) <= 120),
 signature text NULL,
 confirmations jsonb NULL,
 content_hash text NULL,
 signed_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 signed_at timestamptz NULL,
 start_date date NULL,
 end_date date NULL,
 renewal_months integer NULL CHECK (renewal_months BETWEEN 1 AND 120),
 renewal_requested_at timestamptz NULL,
 termination_requested_at timestamptz NULL,
 row_version bigint NOT NULL DEFAULT 1,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (org_id, id),
 FOREIGN KEY (org_id, application_id) REFERENCES portal.pub_applications(org_id, id) ON DELETE RESTRICT,
 CHECK ((status = 'pending_sign') = (signed_at IS NULL)),
 CHECK ((signed_at IS NULL) OR (signature IS NOT NULL AND signer_name IS NOT NULL AND content_hash IS NOT NULL
        AND start_date IS NOT NULL AND end_date IS NOT NULL)),
 CHECK ((renewal_months IS NULL) = (renewal_requested_at IS NULL))
);
CREATE INDEX pub_contracts_org ON portal.pub_contracts(org_id, created_at DESC);

-- Approval now means "the contract is ready to sign" — the notice says so and
-- links to the contracts screen. The other notices stay as in 0167.
CREATE OR REPLACE FUNCTION portal.pub_work_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text := '‘' || left(NEW.title, 80) || '’';
BEGIN
 IF NEW.registration IS NOT DISTINCT FROM OLD.registration THEN RETURN NEW; END IF;
 IF NEW.registration = 'internal_review' AND OLD.registration <> 'needs_fix' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 저작권 관리 신청이 접수됐어요.',
   '담당자가 참여자와 지분을 확인하고 있어요.', '/works/' || NEW.id);
 ELSIF NEW.registration = 'needs_fix' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '에 저작권 보완 요청이 있어요.',
   left(coalesce(NEW.fix->>'reason', '곡 화면에서 요청 내용을 확인해 주세요.'), 300), '/works/' || NEW.id);
 ELSIF NEW.registration = 'ready' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 저작권 관리 신청이 승인됐어요.',
   '저작권 관리 계약서를 확인하고 서명해 주세요.', '/contracts');
 ELSIF NEW.registration = 'registered' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 저작권 등록이 끝났어요.',
   '이제 관리 단체가 나눠 주는 저작권료를 받을 수 있어요.', '/works/' || NEW.id);
 ELSIF NEW.registration = 'rejected' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 저작권 관리 신청이 반려됐어요.',
   '곡 화면에서 사유를 확인해 주세요.', '/works/' || NEW.id);
 END IF;
 RETURN NEW;
END $$;
