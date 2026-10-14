-- 0121: contract end and rights expiry (crates/core/src/contract_terms.rs).
--
-- A release's distribution ends on a date: the artist ends the contract for
-- it or a licensed master's rights expire (at least 30 / 1 day ahead), or
-- staff terminate it. An org-wide contract end schedules every release and
-- stops new submissions. 30 days before, the org is told; on the date no
-- new delivery goes out, every platform still carrying the release gets a
-- takedown, and final settlement is due within 90 days, which staff close.
CREATE TABLE portal.release_terms (
 release_id uuid PRIMARY KEY REFERENCES catalog.releases ON DELETE RESTRICT,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 end_date date NOT NULL,
 reason text NOT NULL CHECK (reason IN ('CONTRACT_END','LICENSE_EXPIRY','TERMINATION','ORG_CONTRACT_END')),
 note text NOT NULL DEFAULT '' CHECK (length(note) <= 1000),
 status text NOT NULL DEFAULT 'SCHEDULED' CHECK (status IN ('SCHEDULED','ENDED','FINAL_SETTLED','CANCELLED')),
 set_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 set_by_staff boolean NOT NULL DEFAULT false,
 warned_at timestamptz NULL,
 ended_at timestamptz NULL,
 final_settlement_due date NULL,
 final_settled_at timestamptz NULL,
 final_note text NOT NULL DEFAULT '' CHECK (length(final_note) <= 1000),
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK ((status IN ('ENDED','FINAL_SETTLED')) = (ended_at IS NOT NULL) OR status = 'CANCELLED')
);
CREATE INDEX release_terms_due ON portal.release_terms(end_date) WHERE status = 'SCHEDULED';
CREATE INDEX release_terms_org ON portal.release_terms(org_id);

CREATE TABLE portal.org_contract_terms (
 org_id uuid PRIMARY KEY REFERENCES identity.orgs ON DELETE RESTRICT,
 end_date date NOT NULL,
 note text NOT NULL DEFAULT '' CHECK (length(note) <= 1000),
 status text NOT NULL DEFAULT 'SCHEDULED' CHECK (status IN ('SCHEDULED','ENDED','CANCELLED')),
 set_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 set_by_staff boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);

-- Whether a release's distribution has ended (the delivery gate; the worker
-- reads it without a grant on the table).
CREATE FUNCTION portal.release_term_ended(p_org uuid, p_release uuid)
RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT EXISTS (
  SELECT 1 FROM portal.release_terms t
   WHERE t.org_id = p_org AND t.release_id = p_release AND t.status <> 'CANCELLED'
     AND t.end_date <= (now() AT TIME ZONE 'Asia/Seoul')::date)
$$;
REVOKE ALL ON FUNCTION portal.release_term_ended(uuid, uuid) FROM PUBLIC;
ALTER TABLE portal.org_contract_terms ADD COLUMN warned_at timestamptz NULL;

-- Notices. Releases ended by an org-wide contract end are told once, by the
-- org's own row.
CREATE FUNCTION portal.release_term_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text; link text;
BEGIN
 IF NEW.reason = 'ORG_CONTRACT_END' AND NEW.status IN ('SCHEDULED','ENDED') THEN RETURN NEW; END IF;
 SELECT coalesce(nullif(btrim(title), ''), '발매') INTO t FROM catalog.releases WHERE id = NEW.release_id;
 link := '/releases/' || NEW.release_id;
 IF NEW.status = 'SCHEDULED' AND (TG_OP = 'INSERT' OR OLD.status <> 'SCHEDULED' OR OLD.end_date <> NEW.end_date) THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 배급 종료가 ' || NEW.end_date || '로 정해졌어요.',
   CASE WHEN NEW.set_by_staff THEN left(coalesce(nullif(NEW.note, ''), '담당자가 정했어요.'), 300)
        ELSE '그날 모든 플랫폼에서 내려가요. 그 전까지는 발매 상세에서 취소할 수 있어요.' END, link);
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'SCHEDULED' AND OLD.warned_at IS NULL AND NEW.warned_at IS NOT NULL THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 배급이 ' || NEW.end_date || '에 끝나요.',
   '그날 모든 플랫폼에서 내려가요. 계속 배급하려면 발매 상세에서 종료를 취소해 주세요.', link);
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'ENDED' AND OLD.status <> 'ENDED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 배급이 끝났어요.',
   '모든 플랫폼에 내려 달라고 요청했어요. 남은 수익은 ' || NEW.final_settlement_due || '까지 최종 정산해요.', link);
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'FINAL_SETTLED' AND OLD.status <> 'FINAL_SETTLED' THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', t || ' 최종 정산을 마쳤어요.',
   left(coalesce(nullif(NEW.final_note, ''), '남은 금액은 정산·지급에서 확인해 주세요.'), 300), '/settlement');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER release_terms_notice AFTER INSERT OR UPDATE ON portal.release_terms
 FOR EACH ROW EXECUTE FUNCTION portal.release_term_notice();

CREATE FUNCTION portal.org_contract_term_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF NEW.status = 'SCHEDULED' AND (TG_OP = 'INSERT' OR OLD.status <> 'SCHEDULED' OR OLD.end_date <> NEW.end_date) THEN
  PERFORM portal.notify(NEW.org_id, 'ACCOUNT', '배급 계약 종료가 ' || NEW.end_date || '로 정해졌어요.',
   '그날 모든 발매가 플랫폼에서 내려가고, 그 뒤 90일 안에 최종 정산해요. 새 발매 신청은 받지 않아요.', '/contracts');
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'SCHEDULED' AND OLD.warned_at IS NULL AND NEW.warned_at IS NOT NULL THEN
  PERFORM portal.notify(NEW.org_id, 'ACCOUNT', '배급 계약이 ' || NEW.end_date || '에 끝나요.',
   '그날 모든 발매가 플랫폼에서 내려가요. 계약 화면에서 종료를 취소할 수 있어요.', '/contracts');
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'ENDED' AND OLD.status <> 'ENDED' THEN
  PERFORM portal.notify(NEW.org_id, 'ACCOUNT', '배급 계약이 끝났어요.',
   '모든 발매를 플랫폼에서 내려 달라고 요청했어요. 남은 수익은 90일 안에 최종 정산해요.', '/contracts');
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'CANCELLED' AND OLD.status <> 'CANCELLED' THEN
  PERFORM portal.notify(NEW.org_id, 'ACCOUNT', '배급 계약 종료를 취소했어요.', '배급이 계속돼요.', '/contracts');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER org_contract_terms_notice AFTER INSERT OR UPDATE ON portal.org_contract_terms
 FOR EACH ROW EXECUTE FUNCTION portal.org_contract_term_notice();
