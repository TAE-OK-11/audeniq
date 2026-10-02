-- Inherit internal priority when a pipeline job is retried, and discard a
-- cancelled purchase's boost at requeue/claim without touching running work.
CREATE OR REPLACE FUNCTION operations.apply_addon_priority() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE boost integer;
BEGIN
 IF NEW.queue NOT IN ('qc','rights','distribution','delivery') THEN RETURN NEW; END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.status<>'QUEUED' THEN RETURN NEW; END IF;
 ELSE
  IF NEW.status NOT IN ('QUEUED','RUNNING') OR OLD.status=NEW.status THEN RETURN NEW; END IF;
 END IF;
 SELECT priority INTO boost FROM catalog.addon_release_priorities WHERE release_id=NEW.release_id;
 IF boost IS NULL AND NEW.addon_priority_previous IS NOT NULL THEN
  -- A later operator-set URGENT priority belongs to the operator.
  IF NEW.priority=10 THEN NEW.priority:=NEW.addon_priority_previous; END IF;
  NEW.addon_priority_previous:=NULL;
 ELSIF boost IS NOT NULL AND NEW.priority<boost THEN
  NEW.addon_priority_previous:=coalesce(NEW.addon_priority_previous,NEW.priority);
  NEW.priority:=boost;
 END IF;
 RETURN NEW;
END $$;
DROP TRIGGER zz_addon_priority ON operations.jobs;
CREATE TRIGGER zz_addon_priority BEFORE INSERT OR UPDATE OF status ON operations.jobs
FOR EACH ROW EXECUTE FUNCTION operations.apply_addon_priority();

-- AFTER prevents an idempotent INSERT ... ON CONFLICT retry from auditing a
-- proposed job that was never inserted. Explicit administrator updates keep
-- their existing actor-aware audit in the application transaction.
CREATE FUNCTION operations.audit_addon_priority_inheritance() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE previous integer; audit_org uuid; addon_order uuid;
BEGIN
 IF NEW.queue NOT IN ('qc','rights','distribution','delivery') THEN RETURN NEW; END IF;
 IF TG_OP='UPDATE' AND OLD.addon_priority_previous IS NULL AND NEW.addon_priority_previous IS NULL THEN RETURN NEW; END IF;
 IF TG_OP='INSERT' THEN previous:=NEW.addon_priority_previous;
 ELSE previous:=OLD.priority;
 END IF;
 IF previous IS NULL OR previous=NEW.priority THEN RETURN NEW; END IF;
 SELECT org_id,addon_order_id INTO audit_org,addon_order
 FROM catalog.addon_release_priorities WHERE release_id=NEW.release_id;
 IF audit_org IS NULL THEN
  SELECT org_id INTO audit_org FROM catalog.releases WHERE id=NEW.release_id;
 END IF;
 INSERT INTO operations.audit_events(id,actor_service,org_id,resource_id,action,reason_code,request_id,before_value,after_value)
 VALUES(gen_random_uuid(),'audeniq-system',audit_org,NEW.id,'addon.queue.priority','QUEUE_PRIORITY_RECONCILED',gen_random_uuid(),
  jsonb_build_object('priority',previous,'release_id',NEW.release_id),
  jsonb_build_object('priority',NEW.priority,'release_id',NEW.release_id,'order_id',addon_order));
 RETURN NEW;
END $$;
CREATE TRIGGER addon_priority_inheritance_audit AFTER INSERT OR UPDATE OF status ON operations.jobs
FOR EACH ROW EXECUTE FUNCTION operations.audit_addon_priority_inheritance();

-- Forward-only repair for deployments that already applied 0069. Preserve
-- the original submission date and snapshot; do not restart an entitlement.
WITH previous AS (
 SELECT id FROM catalog.addon_orders WHERE payment_status='NOT_REQUIRED'
 AND valid_until IS NULL AND submitted_at IS NOT NULL
 AND (validity_months_snapshot IS NOT NULL OR validity_days_snapshot IS NOT NULL)
 FOR UPDATE
), repaired AS (
 UPDATE catalog.addon_orders o SET valid_until=submitted_at+CASE
  WHEN validity_months_snapshot IS NOT NULL THEN make_interval(months=>validity_months_snapshot)
  ELSE make_interval(days=>validity_days_snapshot) END,row_version=row_version+1
 FROM previous p WHERE o.id=p.id RETURNING o.id,o.org_id,o.valid_until
)
INSERT INTO operations.audit_events(id,actor_service,org_id,resource_id,action,reason_code,request_id,before_value,after_value)
SELECT gen_random_uuid(),'audeniq-migration',org_id,id,'addon.entitlement.repaired','ADDON_STABILIZATION',gen_random_uuid(),
 jsonb_build_object('valid_until',NULL),jsonb_build_object('valid_until',valid_until) FROM repaired;

WITH previous AS (
 SELECT id,priority FROM catalog.addon_orders
 WHERE service_code='PRIORITY_DELIVERY' AND status IN ('CANCELLED','REJECTED','FAILED') AND priority<>0 FOR UPDATE
), repaired AS (
 UPDATE catalog.addon_orders o SET priority=0,row_version=row_version+1
 FROM previous p WHERE o.id=p.id RETURNING o.id,o.org_id,p.priority
)
INSERT INTO operations.audit_events(id,actor_service,org_id,resource_id,action,reason_code,request_id,before_value,after_value)
SELECT gen_random_uuid(),'audeniq-migration',org_id,id,'addon.priority.removed','ADDON_STABILIZATION',gen_random_uuid(),
 jsonb_build_object('priority',priority),jsonb_build_object('priority',0) FROM repaired;
-- SELECT ... FOR SHARE requires UPDATE privileges. Keep staff grants read-only
-- for the API while allowing this narrow, read-only authorization lock.
CREATE FUNCTION identity.lock_active_staff_role(requested_user uuid) RETURNS text
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT sm.role FROM identity.staff_members sm JOIN identity.users u ON u.id=sm.user_id
 WHERE sm.user_id=requested_user AND sm.status='ACTIVE' AND u.status='ACTIVE'
 FOR SHARE OF sm,u
$$;
REVOKE ALL ON FUNCTION identity.lock_active_staff_role(uuid) FROM PUBLIC;
