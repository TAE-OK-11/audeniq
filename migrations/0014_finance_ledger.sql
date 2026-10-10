-- F7: finance ledger core (BLUEPRINT §8, INV-09). Partner-dependent parts
-- (report parsers, auto-matching) are F6; the tables exist per the F0 data
-- contract but have no parser/matcher logic behind them yet.
--
-- Immutability stance: ledger_transactions, ledger_entries,
-- commercial_split_snapshots, royalty_reports, report_lines are evidence and
-- reject UPDATE/DELETE. Corrections are reversal transactions only.
-- payout_orders, finance_holds, royalty_match_candidates are workflow tables
-- with explicit status lifecycles, so they stay mutable.

-- Double-entry ledger: one transaction, one currency, balanced sides.
CREATE TABLE finance.ledger_transactions (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 transaction_code text NOT NULL CHECK(length(btrim(transaction_code))>0),
 currency char(3) NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 status text NOT NULL DEFAULT 'POSTED' CHECK(status IN ('POSTED','REVERSED')),
 fx_rate numeric NULL CHECK(fx_rate IS NULL OR fx_rate > 0),
 fee_policy_version text NULL,
 contract_version text NULL,
 tax_rule_version text NULL,
 reversal_of uuid NULL REFERENCES finance.ledger_transactions ON DELETE RESTRICT,
 source_ref jsonb NOT NULL DEFAULT '{}',
 description text NOT NULL DEFAULT '',
 created_by uuid NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,id)
);
CREATE INDEX ledger_transactions_org ON finance.ledger_transactions(org_id,created_at);
CREATE INDEX ledger_transactions_reversal ON finance.ledger_transactions(org_id,reversal_of) WHERE reversal_of IS NOT NULL;

CREATE TABLE finance.ledger_entries (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 transaction_id uuid NOT NULL REFERENCES finance.ledger_transactions ON DELETE RESTRICT,
 account text NOT NULL CHECK(length(btrim(account))>0),
 side text NOT NULL CHECK(side IN ('DEBIT','CREDIT')),
 amount numeric NOT NULL CHECK(amount >= 0),
 currency char(3) NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 party_id uuid NULL,
 isrc text NULL,
 split_snapshot_id uuid NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,id)
);
CREATE INDEX ledger_entries_tx ON finance.ledger_entries(org_id,transaction_id);
CREATE INDEX ledger_entries_party ON finance.ledger_entries(org_id,party_id) WHERE party_id IS NOT NULL;
CREATE INDEX ledger_entries_isrc ON finance.ledger_entries(org_id,isrc) WHERE isrc IS NOT NULL;

-- Commercial split snapshots: recipient shares pinned to a validity window.
-- New contracts create a new window; past windows are never rewritten.
CREATE TABLE finance.commercial_split_snapshots (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 valid_from timestamptz NOT NULL,
 valid_to timestamptz NULL CHECK(valid_to IS NULL OR valid_to > valid_from),
 lines jsonb NOT NULL,
 contract_version text NOT NULL,
 source_ref jsonb NOT NULL DEFAULT '{}',
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,id),
 -- Append-only versioning: a new contract means a new row with a later
 -- valid_from. "Effective at t" = the row with the greatest valid_from <= t,
 -- so windows never need closing (rows are immutable).
 UNIQUE(org_id,release_id,valid_from)
);
CREATE INDEX split_snapshots_release ON finance.commercial_split_snapshots(org_id,release_id,valid_from);

-- Payout orders: manual approval by default. Status lifecycle is explicit;
-- bank responses that are unclear stay SUBMITTED_UNKNOWN (never auto-retried).
CREATE TABLE finance.payout_orders (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 idempotency_key text NOT NULL,
 payee_party_id uuid NOT NULL,
 amount numeric NOT NULL CHECK(amount > 0),
 currency char(3) NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 status text NOT NULL DEFAULT 'PENDING_APPROVAL'
   CHECK(status IN ('PENDING_APPROVAL','APPROVED','SUBMITTED','SUBMITTED_UNKNOWN','SETTLED','FAILED','RETURNED','CANCELLED')),
 auto_payout_enabled boolean NOT NULL DEFAULT false,
 approved_by uuid NULL,
 approved_at timestamptz NULL,
 executed_by uuid NULL,
 bank_transaction_id text NULL,
 bank_result jsonb NULL,
 related_transaction_id uuid NULL REFERENCES finance.ledger_transactions ON DELETE RESTRICT,
 created_by uuid NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,idempotency_key)
);
CREATE INDEX payout_orders_payee ON finance.payout_orders(org_id,payee_party_id,status);

-- Finance holds: scoped (ISRC/RELEASE/PARTY), never blanket-expanded.
CREATE TABLE finance.finance_holds (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 scope_type text NOT NULL CHECK(scope_type IN ('ISRC','RELEASE','PARTY')),
 scope_value text NOT NULL CHECK(length(btrim(scope_value))>0),
 reason text NOT NULL CHECK(length(btrim(reason))>0),
 reason_class text NOT NULL CHECK(reason_class IN ('RIGHTS_DISPUTE','TAKEDOWN','CONTRACT_WITHDRAWAL','FRAUD_REVIEW','OTHER')),
 active boolean NOT NULL DEFAULT true,
 created_by uuid NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 released_at timestamptz NULL,
 released_by uuid NULL,
 UNIQUE(org_id,id)
);
CREATE INDEX finance_holds_scope ON finance.finance_holds(org_id,scope_type,scope_value) WHERE active;

-- Report intake (F0 data contract; parsers are F6).
CREATE TABLE finance.royalty_reports (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 dsp_id text NOT NULL CHECK(length(btrim(dsp_id))>0),
 period_start date NOT NULL,
 period_end date NOT NULL CHECK(period_end >= period_start),
 source_filename text NOT NULL,
 source_hash text NOT NULL CHECK(source_hash ~ '^[a-f0-9]{64}$'),
 currency char(3) NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 status text NOT NULL DEFAULT 'RECEIVED' CHECK(status IN ('RECEIVED','NORMALIZED','MATCHED','POSTED')),
 raw_ref jsonb NOT NULL DEFAULT '{}',
 received_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,source_hash),
 UNIQUE(org_id,id)
);
CREATE INDEX royalty_reports_dsp ON finance.royalty_reports(org_id,dsp_id,period_start);

CREATE TABLE finance.report_lines (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 report_id uuid NOT NULL REFERENCES finance.royalty_reports ON DELETE RESTRICT,
 line_no integer NOT NULL CHECK(line_no >= 0),
 isrc text NULL,
 dsp_track_id text NULL,
 quantity numeric NULL CHECK(quantity IS NULL OR quantity >= 0),
 gross_amount numeric NULL CHECK(gross_amount IS NULL OR gross_amount >= 0),
 currency char(3) NULL CHECK(currency IS NULL OR currency ~ '^[A-Z]{3}$'),
 match_status text NOT NULL DEFAULT 'UNMATCHED' CHECK(match_status IN ('AUTO','MANUAL','UNMATCHED')),
 matched_release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 match_evidence jsonb NULL,
 raw jsonb NOT NULL DEFAULT '{}',
 UNIQUE(org_id,id),
 UNIQUE(report_id,line_no)
);
CREATE INDEX report_lines_match ON finance.report_lines(org_id,match_status);

-- Ambiguous-match candidates: evidence only, no auto-posting (INV-09).
CREATE TABLE finance.royalty_match_candidates (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 report_line_id uuid NOT NULL REFERENCES finance.report_lines ON DELETE RESTRICT,
 release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 dsp_id text NULL,
 binding_ref jsonb NULL,
 split_snapshot_id uuid NULL REFERENCES finance.commercial_split_snapshots ON DELETE RESTRICT,
 evidence jsonb NOT NULL,
 score numeric NULL,
 status text NOT NULL DEFAULT 'PENDING' CHECK(status IN ('PENDING','ACCEPTED','REJECTED')),
 reviewed_by uuid NULL,
 reviewed_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,id)
);
CREATE INDEX match_candidates_line ON finance.royalty_match_candidates(org_id,report_line_id,status);

DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY[
   'finance.ledger_transactions',
   'finance.ledger_entries',
   'finance.commercial_split_snapshots',
   'finance.royalty_reports',
   'finance.report_lines'
 ] LOOP
  EXECUTE format('CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON %s FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation()',t);
 END LOOP;
END $$;
-- Deliberately no runtime grants for the new tables (same stance as F2/F3/F4):
-- deploy/grants.sql stays closed until the runtime-role hardening pass.
