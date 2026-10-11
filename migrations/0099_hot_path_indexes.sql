-- 0099: 자주 도는 조회의 인덱스 (백엔드 점검).
--
-- Each index matches a query that runs on every request, every live tick or
-- every staff poll and so far scanned the whole table:
-- * balances / statements: ROYALTY_PAYABLE sum per org and currency;
-- * staff payout queue and overview: payout requests by status;
-- * live streams (every 3 s per open tab): latest inquiry / message / task /
--   note change and the open-inquiry count;
-- * work exceptions (inbox and summary): dead-lettered and retrying jobs;
-- * stage 2 overrides: jobs pinned to a revision;
-- * delivery reconcile: open send jobs per delivery job.
CREATE INDEX ledger_entries_account ON finance.ledger_entries(org_id, account, currency) INCLUDE (side, amount);
CREATE INDEX payout_requests_status ON portal.payout_requests(status, created_at);

CREATE INDEX inquiries_updated ON portal.inquiries(updated_at);
CREATE INDEX inquiries_open ON portal.inquiries(status) WHERE status = 'OPEN';
CREATE INDEX inquiry_messages_latest ON portal.inquiry_messages(created_at DESC);
CREATE INDEX tasks_updated ON work.tasks(updated_at);
CREATE INDEX notes_touched ON work.notes((GREATEST(created_at, edited_at, resolved_at, archived_at)));

CREATE INDEX jobs_dead_letter ON operations.jobs(dead_lettered_at DESC) WHERE status = 'DEAD_LETTER';
CREATE INDEX jobs_retrying ON operations.jobs(created_at DESC)
 WHERE status IN ('QUEUED','RUNNING') AND last_error IS NOT NULL;
CREATE INDEX jobs_pinned_revision ON operations.jobs(pinned_revision_id, created_at DESC)
 WHERE pinned_revision_id IS NOT NULL;
CREATE INDEX jobs_delivery_send ON operations.jobs((payload->>'delivery_job_id'))
 WHERE kind = 'delivery.send' AND status IN ('QUEUED','RUNNING');
