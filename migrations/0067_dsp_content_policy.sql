-- Rule 2 automatic approvals include the source-backed content policy.
-- Old approvals keep their recorded authority; a re-stage re-evaluates
-- them under the current application/rights/consent and content rules.
ALTER TABLE distribution.delivery_staging DROP CONSTRAINT delivery_approval_provenance;
ALTER TABLE distribution.delivery_staging ADD CONSTRAINT delivery_approval_provenance CHECK (
 (approval_rule_version IS NULL OR approval_rule_version IN ('1','2','STAFF_FINAL')) AND
 (approval_rule_version IS NULL OR approval='APPROVED') AND
 (approval <> 'APPROVED' OR
   (readiness <> 'CONTENT_BLOCKED' AND approval_by IS NOT NULL AND approval_rule_version IS NULL) OR
   COALESCE((readiness <> 'CONTENT_BLOCKED' AND approval_by IS NULL AND approval_rule_version='STAFF_FINAL'),false) OR
   COALESCE((approval_by IS NULL AND approval_rule_version IN ('1','2') AND approval_at IS NOT NULL
    AND readiness='READY' AND route_status='ROUTABLE' AND NOT ern_is_preview
    AND distribution.automatic_checks_clear(checks)),false))
);
