-- Fixed slots bound the entire durable image backlog, including terminal rows.
-- Upload completion only replaces one free/old queued slot; never waits for a
-- codec, advisory lock, or a row currently held by another upload/worker.
CREATE TABLE catalog.image_jobs (
 slot smallint PRIMARY KEY CHECK (slot BETWEEN 1 AND 256),
 org_id uuid,
 asset_id uuid,
 source_sha256 text CHECK (source_sha256 ~ '^[0-9a-f]{64}$'),
 status text NOT NULL DEFAULT 'EMPTY'
   CHECK (status IN ('EMPTY','QUEUED','RUNNING','DONE','SKIPPED','FAILED')),
 attempts smallint NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
 run_at timestamptz NOT NULL DEFAULT now(),
 queued_at timestamptz NOT NULL DEFAULT now(),
 outcome text,
 FOREIGN KEY (org_id,asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT,
 CHECK ((status='EMPTY') = (org_id IS NULL AND asset_id IS NULL AND source_sha256 IS NULL)),
 CHECK (status='EMPTY' OR (org_id IS NOT NULL AND asset_id IS NOT NULL AND source_sha256 IS NOT NULL))
);
INSERT INTO catalog.image_jobs(slot) SELECT generate_series(1,256);

CREATE FUNCTION catalog.queue_image_derivatives() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
DECLARE target smallint;
BEGIN
 IF NEW.kind <> 'IMAGE' OR NEW.state <> 'REGISTERED'
    OR NEW.content_type NOT IN ('image/jpeg','image/png') OR NEW.sha256 IS NULL THEN
   RETURN NEW;
 END IF;
 IF TG_OP='UPDATE' THEN
   IF OLD.state=NEW.state AND OLD.sha256 IS NOT DISTINCT FROM NEW.sha256 THEN
     RETURN NEW;
   END IF;
 END IF;
 -- Prefer unused/terminal slots, otherwise evict the oldest waiting image.
 -- A RUNNING image is never evicted. SKIP LOCKED also handles upload bursts.
 SELECT slot INTO target FROM catalog.image_jobs
 WHERE status <> 'RUNNING'
 ORDER BY (status='QUEUED'), queued_at, slot
 LIMIT 1 FOR UPDATE SKIP LOCKED;
 IF target IS NULL THEN
   RAISE LOG 'image_queue_saturated';
   RETURN NEW;
 END IF;
 UPDATE catalog.image_jobs SET org_id=NEW.org_id,asset_id=NEW.id,
   source_sha256=NEW.sha256,status='QUEUED',attempts=0,
   run_at=clock_timestamp(),queued_at=clock_timestamp(),outcome=NULL
 WHERE slot=target;
 RETURN NEW;
EXCEPTION WHEN OTHERS THEN
 -- Optional derivatives must never fail a successful source upload.
 -- Do not log SQLERRM: driver messages may contain sensitive values.
 RAISE LOG 'image_queue_unavailable';
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION catalog.queue_image_derivatives() FROM PUBLIC;
CREATE TRIGGER queue_image_derivatives AFTER INSERT OR UPDATE OF state,sha256
 ON catalog.assets FOR EACH ROW EXECUTE FUNCTION catalog.queue_image_derivatives();
