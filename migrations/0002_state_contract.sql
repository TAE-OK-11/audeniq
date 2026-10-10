CREATE TABLE operations.allowed_transitions (axis text NOT NULL, old_status text NOT NULL, new_status text NOT NULL, PRIMARY KEY(axis,old_status,new_status));
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','DRAFT','SUBMITTED');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','SUBMITTED','STAGE1_RUNNING');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE1_RUNNING','STAGE1_CORRECTION');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE1_CORRECTION','SUBMITTED');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE1_RUNNING','STAGE1_PASSED');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE1_PASSED','STAGE2_RUNNING');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_RUNNING','STAGE2_REVIEW');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_RUNNING','STAGE2_CORRECTION');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_REVIEW','STAGE2_RUNNING');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_CORRECTION','SUBMITTED');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_RUNNING','STAGE2_PASSED');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_PASSED','STAGE3_PREPARING');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE3_PREPARING','STAGE3_CORRECTION');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE3_CORRECTION','STAGE3_PREPARING');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE3_PREPARING','READY_FOR_DELIVERY');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','READY_FOR_DELIVERY','ON_HOLD_RIGHTS');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_RUNNING','ON_HOLD_RIGHTS');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','ON_HOLD_RIGHTS','STAGE2_RUNNING');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','READY_FOR_DELIVERY','SUPERSEDED');
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','SUBMITTED','WITHDRAWN');
INSERT INTO operations.allowed_transitions VALUES ('delivery_job_status','QUEUED','LEASED');
INSERT INTO operations.allowed_transitions VALUES ('delivery_job_status','LEASED','CANCELLED_RIGHTS');
INSERT INTO operations.allowed_transitions VALUES ('delivery_job_status','LEASED','SENT_UNKNOWN');
INSERT INTO operations.allowed_transitions VALUES ('delivery_job_status','LEASED','ACCEPTED');
INSERT INTO operations.allowed_transitions VALUES ('delivery_job_status','SENT_UNKNOWN','ACCEPTED');
INSERT INTO operations.allowed_transitions VALUES ('delivery_job_status','SENT_UNKNOWN','REJECTED');
INSERT INTO operations.allowed_transitions VALUES ('dsp_live_status','NOT_SUBMITTED','IN_REVIEW');
INSERT INTO operations.allowed_transitions VALUES ('dsp_live_status','IN_REVIEW','LIVE');
INSERT INTO operations.allowed_transitions VALUES ('dsp_live_status','LIVE','TAKEN_DOWN');
ALTER TABLE catalog.releases ADD CONSTRAINT pipeline_status CHECK(status IN ('DRAFT','SUBMITTED','STAGE1_RUNNING','STAGE1_CORRECTION','STAGE1_PASSED','STAGE2_RUNNING','STAGE2_REVIEW','STAGE2_CORRECTION','STAGE2_PASSED','STAGE3_PREPARING','STAGE3_CORRECTION','READY_FOR_DELIVERY','ON_HOLD_RIGHTS','SUPERSEDED','WITHDRAWN'));
CREATE FUNCTION catalog.guard_pipeline() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.status<>OLD.status THEN
  IF NOT EXISTS(SELECT 1 FROM operations.allowed_transitions WHERE axis='application_pipeline_status' AND old_status=OLD.status AND new_status=NEW.status) THEN
   RAISE EXCEPTION 'forbidden pipeline transition' USING ERRCODE='23514'; END IF;
  -- Foundation has no legal consent, QC, or rights authorizer. All public stage progression is closed.
  RAISE EXCEPTION 'pipeline execution gated until F2' USING ERRCODE='23514';
 END IF;
 IF NEW.row_version<>OLD.row_version+1 THEN RAISE EXCEPTION 'row version must increment' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER pipeline_transition BEFORE UPDATE ON catalog.releases FOR EACH ROW EXECUTE FUNCTION catalog.guard_pipeline();
