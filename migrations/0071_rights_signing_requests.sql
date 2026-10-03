-- 0071: 권리 서류 서명 요청 (권리자 본인 확인 + 자필 서명).
--
-- The artist prepares a rights document and opens a signing request. The
-- rights holder signs it themselves, either through a link the artist sends
-- (LINK) or on the artist's device handed over in person (IN_PERSON). Either
-- way the holder first passes identity verification with an external
-- provider, then reads the document, gives the consents and draws their own
-- signature. Only then is a RIGHTS_PROOF document created (form 2.0).
--
-- The link token is never stored: only its SHA-256. The verified identity is
-- kept to what proves who signed (name, birth date, provider transaction and
-- a peppered hash of the provider's CI), never the raw CI or phone number.
-- Every step is an append-only event whose hash chains to the previous one,
-- and the final certificate hash covers the document, signature, identity and
-- the event chain head.
CREATE TABLE portal.signing_requests (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 document_no uuid NOT NULL,
 form text NOT NULL CHECK(form = 'AUD-RIGHTS 2.0'),
 document_kind text NOT NULL CHECK(document_kind IN ('master','artwork','composition','sample','performer','shared')),
 title text NOT NULL CHECK(length(btrim(title)) BETWEEN 1 AND 200),
 body text NOT NULL CHECK(length(btrim(body)) BETWEEN 1 AND 20000),
 body_hash text NOT NULL CHECK(body_hash ~ '^[0-9a-f]{64}$'),
 rights_holder text NOT NULL CHECK(length(btrim(rights_holder)) BETWEEN 1 AND 120),
 signer_name text NOT NULL CHECK(length(btrim(signer_name)) BETWEEN 1 AND 120),
 signer_role text NOT NULL CHECK(signer_role IN ('권리자 본인','권리자의 위임을 받은 대리인','법인 대표자')),
 channel text NOT NULL CHECK(channel IN ('LINK','IN_PERSON')),
 token_hash bytea NOT NULL UNIQUE,
 status text NOT NULL DEFAULT 'PENDING' CHECK(status IN ('PENDING','VERIFIED','SIGNED','DECLINED','CANCELLED')),
 expires_at timestamptz NOT NULL,
 identity jsonb NULL,
 signature text NOT NULL DEFAULT '' CHECK(signature = '' OR (signature LIKE 'data:image/png;base64,%' AND length(signature) <= 60000)),
 consents jsonb NULL,
 signed_at timestamptz NULL,
 decline_reason text NOT NULL DEFAULT '' CHECK(length(decline_reason) <= 500),
 document_id uuid NULL REFERENCES portal.documents ON DELETE RESTRICT,
 certificate_hash text NULL CHECK(certificate_hash IS NULL OR certificate_hash ~ '^[0-9a-f]{64}$'),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id, document_no),
 CHECK(status NOT IN ('VERIFIED','SIGNED') OR identity IS NOT NULL),
 CHECK(status <> 'SIGNED' OR (signature <> '' AND signed_at IS NOT NULL AND consents IS NOT NULL
   AND document_id IS NOT NULL AND certificate_hash IS NOT NULL))
);
CREATE INDEX signing_requests_release ON portal.signing_requests(org_id, release_id, created_at DESC);

CREATE TABLE portal.signing_events (
 id bigserial PRIMARY KEY,
 request_id uuid NOT NULL REFERENCES portal.signing_requests ON DELETE RESTRICT,
 event text NOT NULL CHECK(event IN ('CREATED','LINK_REISSUED','VIEWED','IDENTITY_VERIFIED','IDENTITY_FAILED','SIGNED','DECLINED','CANCELLED')),
 at timestamptz NOT NULL DEFAULT now(),
 actor_user_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 ip_hash text NULL,
 user_agent text NOT NULL DEFAULT '' CHECK(length(user_agent) <= 300),
 detail jsonb NOT NULL DEFAULT '{}'::jsonb,
 prev_hash text NULL,
 hash text NOT NULL CHECK(hash ~ '^[0-9a-f]{64}$')
);
CREATE INDEX signing_events_request ON portal.signing_events(request_id, id);

-- Events are evidence: no edits, no deletes.
CREATE FUNCTION portal.signing_events_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'signing events are append-only' USING ERRCODE = '23514';
END $$;
CREATE TRIGGER signing_events_no_change BEFORE UPDATE OR DELETE ON portal.signing_events
  FOR EACH ROW EXECUTE FUNCTION portal.signing_events_append_only();

-- Signed rights documents now come from a verified signing request (form 2.0).
-- Form 1.0 rows stay valid as history; new 1.0 records are refused by the API.
ALTER TABLE portal.documents DROP CONSTRAINT electronic_rights_signed;
ALTER TABLE portal.documents ADD CONSTRAINT electronic_rights_signed CHECK (
  electronic_record IS NULL OR (
    kind = 'RIGHTS_PROOF' AND signed_at IS NOT NULL AND signed_by IS NOT NULL
    AND signature <> '' AND checked_at IS NOT NULL
    AND electronic_record->>'form' IN ('AUD-RIGHTS 1.0','AUD-RIGHTS 2.0')
    AND electronic_record->>'content_hash' ~ '^[0-9a-f]{64}$'
    AND electronic_record->>'document_no' IS NOT NULL
    AND (electronic_record->>'form' <> 'AUD-RIGHTS 2.0' OR (
      electronic_record->>'signing_request_id' IS NOT NULL
      AND electronic_record->>'certificate_hash' ~ '^[0-9a-f]{64}$'
      AND electronic_record->'identity'->>'verified_at' IS NOT NULL))
  )
);

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT SELECT, INSERT, UPDATE ON portal.signing_requests TO audeniq_api';
    EXECUTE 'GRANT SELECT, INSERT ON portal.signing_events TO audeniq_api';
    EXECUTE 'GRANT USAGE ON SEQUENCE portal.signing_events_id_seq TO audeniq_api';
  END IF;
END
$$;
