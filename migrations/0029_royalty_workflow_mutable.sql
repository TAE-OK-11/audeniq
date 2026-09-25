-- 0029: Fix royalty report mutability.
--
-- finance.report_lines and finance.royalty_reports were given blanket
-- immutable triggers, but both have workflow columns that must change:
--   report_lines: match_status, matched_release_id, match_evidence
--   royalty_reports: status (RECEIVED -> NORMALIZED -> MATCHED -> POSTED)
--
-- The evidence columns (isrc, quantity, amounts, raw, source_hash, ...)
-- stay immutable. Only the workflow columns may be updated.

CREATE OR REPLACE FUNCTION finance.allow_workflow_update()
RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'immutable record';
    END IF;

    IF TG_TABLE_NAME = 'report_lines' THEN
        -- Evidence columns must not change.
        IF (OLD.id IS DISTINCT FROM NEW.id
            OR OLD.org_id IS DISTINCT FROM NEW.org_id
            OR OLD.report_id IS DISTINCT FROM NEW.report_id
            OR OLD.line_no IS DISTINCT FROM NEW.line_no
            OR OLD.isrc IS DISTINCT FROM NEW.isrc
            OR OLD.dsp_track_id IS DISTINCT FROM NEW.dsp_track_id
            OR OLD.quantity IS DISTINCT FROM NEW.quantity
            OR OLD.gross_amount IS DISTINCT FROM NEW.gross_amount
            OR OLD.currency IS DISTINCT FROM NEW.currency
            OR OLD.raw IS DISTINCT FROM NEW.raw) THEN
            RAISE EXCEPTION 'immutable record: evidence columns cannot change';
        END IF;
    ELSIF TG_TABLE_NAME = 'royalty_reports' THEN
        IF (OLD.id IS DISTINCT FROM NEW.id
            OR OLD.org_id IS DISTINCT FROM NEW.org_id
            OR OLD.dsp_id IS DISTINCT FROM NEW.dsp_id
            OR OLD.period_start IS DISTINCT FROM NEW.period_start
            OR OLD.period_end IS DISTINCT FROM NEW.period_end
            OR OLD.source_filename IS DISTINCT FROM NEW.source_filename
            OR OLD.source_hash IS DISTINCT FROM NEW.source_hash
            OR OLD.currency IS DISTINCT FROM NEW.currency
            OR OLD.raw_ref IS DISTINCT FROM NEW.raw_ref
            OR OLD.received_at IS DISTINCT FROM NEW.received_at) THEN
            RAISE EXCEPTION 'immutable record: evidence columns cannot change';
        END IF;
    ELSE
        RAISE EXCEPTION 'immutable record';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Replace the blanket immutable triggers.
DROP TRIGGER IF EXISTS immutable ON finance.report_lines;
DROP TRIGGER IF EXISTS immutable ON finance.royalty_reports;
DROP TRIGGER IF EXISTS workflow_mutable ON finance.report_lines;
DROP TRIGGER IF EXISTS workflow_mutable ON finance.royalty_reports;

CREATE TRIGGER workflow_mutable BEFORE UPDATE OR DELETE ON finance.report_lines
    FOR EACH ROW EXECUTE FUNCTION finance.allow_workflow_update();
CREATE TRIGGER workflow_mutable BEFORE UPDATE OR DELETE ON finance.royalty_reports
    FOR EACH ROW EXECUTE FUNCTION finance.allow_workflow_update();
