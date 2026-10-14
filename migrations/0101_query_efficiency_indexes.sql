-- 0101: 남은 풀스캔 조회의 인덱스 (백엔드 효율화 점검).
--
-- Found by EXPLAINing every SQL literal in crates/core with sequential scans
-- disabled: each index below backs a query that otherwise scanned the whole
-- table (or a whole unrelated index).
-- * staff overview / release review queue: releases by status across orgs;
-- * GET /api/me (workspace list): memberships of one user;
-- * delivery ACK / poll / mark-live / takedown: the delivery job of one
--   (package, partner) without the org prefix of the unique key;
-- * studio live stream (every 3 s per tab): an org's latest-changed inquiry;
-- * staff inquiry list: inquiries by status ordered by last change.
CREATE INDEX releases_status ON catalog.releases(status);
CREATE INDEX memberships_user ON identity.memberships(user_id);
CREATE INDEX delivery_jobs_package_partner ON execution.delivery_jobs(package_id, partner_id);
CREATE INDEX inquiries_org_updated ON portal.inquiries(org_id, updated_at DESC);
CREATE INDEX inquiries_status_updated ON portal.inquiries(status, updated_at);
