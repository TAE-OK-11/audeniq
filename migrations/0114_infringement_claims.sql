-- 0114: rights-infringement claims against distributed releases
-- (crates/core/src/infringement.rs, LEGAL_AUDIT D12·E12).
--
-- Anyone may file a claim (no account): who they are (sealed), what right,
-- their work and the infringing release, evidence links and a good-faith
-- statement. Staff review it:
--   RECEIVED  → REJECTED   (not actionable; nothing reaches the artist)
--   RECEIVED  → SUSPENDED  (matched to a release; takedown sent to every
--                           platform it reached; the artist is told and may
--                           answer within 30 days)
--   SUSPENDED → COUNTER_NOTICED (the artist's answer)
--   SUSPENDED / COUNTER_NOTICED → RESTORED (claim not upheld; staff re-send)
--                               → UPHELD   (claim upheld; stays down)
-- The artist never sees the claimant's contact details.
CREATE TABLE portal.infringement_claims (
 id uuid PRIMARY KEY,
 claimant_name_cipher bytea NOT NULL,
 claimant_email_cipher bytea NOT NULL,
 claimant_phone_cipher bytea NULL,
 claimant_role text NOT NULL CHECK (claimant_role IN ('RIGHTS_HOLDER','AGENT')),
 right_type text NOT NULL CHECK (right_type IN ('COPYRIGHT','NEIGHBORING','TRADEMARK','PUBLICITY','OTHER')),
 work text NOT NULL CHECK (length(btrim(work)) BETWEEN 1 AND 2000),
 target text NOT NULL CHECK (length(btrim(target)) BETWEEN 1 AND 2000),
 evidence_urls jsonb NOT NULL DEFAULT '[]'::jsonb
   CHECK (jsonb_typeof(evidence_urls) = 'array' AND jsonb_array_length(evidence_urls) <= 5),
 status text NOT NULL DEFAULT 'RECEIVED'
   CHECK (status IN ('RECEIVED','REJECTED','SUSPENDED','COUNTER_NOTICED','RESTORED','UPHELD')),
 org_id uuid NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 staff_note text NOT NULL DEFAULT '' CHECK (length(staff_note) <= 1000),
 decided_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 decided_at timestamptz NULL,
 counter_deadline timestamptz NULL,
 counter_notice text NULL CHECK (counter_notice IS NULL OR length(btrim(counter_notice)) BETWEEN 1 AND 3000),
 counter_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 counter_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK ((org_id IS NULL) = (release_id IS NULL)),
 CHECK (status IN ('RECEIVED','REJECTED') OR release_id IS NOT NULL),
 CHECK ((status = 'RECEIVED') = (decided_at IS NULL)),
 CHECK ((counter_notice IS NULL) = (counter_at IS NULL))
);
CREATE INDEX infringement_claims_queue ON portal.infringement_claims (status, created_at);
CREATE INDEX infringement_claims_release ON portal.infringement_claims (release_id) WHERE release_id IS NOT NULL;

-- The artist hears about a claim only once it touches their release.
CREATE FUNCTION portal.infringement_claim_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text;
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status OR NEW.org_id IS NULL THEN RETURN NEW; END IF;
 SELECT coalesce(nullif(btrim(title), ''), '발매') INTO t FROM catalog.releases WHERE id = NEW.release_id;
 IF NEW.status = 'SUSPENDED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 권리침해 신고로 배급을 중단했어요.',
   '신고 내용을 확인하고 ' || to_char(NEW.counter_deadline AT TIME ZONE 'Asia/Seoul', 'YYYY-MM-DD') || '까지 소명할 수 있어요.',
   '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'RESTORED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 권리침해 신고가 받아들여지지 않았어요.',
   left(coalesce(nullif(NEW.staff_note, ''), '배급을 다시 진행해요. 플랫폼마다 다시 올라가는 데 며칠 걸릴 수 있어요.'), 300),
   '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'UPHELD' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 권리침해 신고가 인정됐어요.',
   left(coalesce(nullif(NEW.staff_note, ''), '배급 중단이 유지돼요. 자세한 내용은 문의로 확인해 주세요.'), 300),
   '/releases/' || NEW.release_id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER infringement_claims_notice AFTER UPDATE OF status ON portal.infringement_claims
 FOR EACH ROW EXECUTE FUNCTION portal.infringement_claim_notice();
