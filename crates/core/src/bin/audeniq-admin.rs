//! Operator CLI. Runs with the schema-owner DATABASE_URL (never a runtime
//! role). Every change is logged with the operator name
//! (`--operator NAME` or `AUDENIQ_OPERATOR`).
//!
//! audeniq-admin external-recording import METADATA_JSON AUDIO_FILE
//! audeniq-admin external-recording list
//! audeniq-admin external-recording activate|deactivate REFERENCE_UUID
//!   (references must be lossless: WAV, FLAC, ALAC, AIFF, WavPack or TTA)
//! audeniq-admin fingerprint backfill [--limit N]
//!   (queues current-version fingerprints for assets that only have older
//!    ones; repeat until it reports 0)
//! audeniq-admin protected list
//! audeniq-admin protected add NAME [--mode CONTAINS|TOKEN] [--action BLOCK|REVIEW] [--note TEXT]
//! audeniq-admin protected remove NAME              (deactivate; history kept)
//! audeniq-admin protected activate NAME
//! audeniq-admin protected alias NAME ALIAS [--phrase] [--mode ..] [--action ..]
//! audeniq-admin protected remove-alias NAME ALIAS
//! audeniq-admin protected grant-exception NAME ORG_ID --reason TEXT
//! audeniq-admin protected revoke-exception NAME ORG_ID
//! audeniq-admin identifier-issuer list
//! audeniq-admin identifier-issuer register UPC|ISRC PREFIX
//!   (GS1 company prefix / ISRC registrant code, e.g. `register ISRC KR-A1B`;
//!    replaces the virtual test range for new codes)
//! audeniq-admin partner list
//! audeniq-admin partner set-dsp PARTNER_ID [DSP_UUID]
//!   (links the partner to the DSP id Stage 2 eligibility keys on)
//! audeniq-admin partner status PARTNER_ID        (onboarding record + gaps)
//! audeniq-admin partner config-check             (PARTNER_CONFIG_DIR files, secrets)
//! audeniq-admin partner probe PARTNER_ID         (connect; records endpoint + credential kind)
//! audeniq-admin partner dpid PARTNER_ID PADPIDA…  (recipient DDEX party id)
//! audeniq-admin partner test-ern PARTNER_ID FILE (XSD + rules; interop proof)
//! audeniq-admin partner test-ack PARTNER_ID FILE (partner's real ACK parses)
//! audeniq-admin partner capabilities PARTNER_ID '{"send_or_publish":true,…}'
//! audeniq-admin partner contract PARTNER_ID REF  (signed contract filing reference)
//! audeniq-admin partner go-live PARTNER_ID       (stage LIVE + delivery_enabled; gaps refuse)
//! audeniq-admin partner suspend PARTNER_ID --reason TEXT
//! audeniq-admin partner route D-5 DIRECT|MERLIN     (contract route per DSP)
//! audeniq-admin partner merlin-eligible D-11 true|false
//!   (Merlin's own agreement: `partner contract merlin REF`)
//! audeniq-admin staff list
//! audeniq-admin staff grant EMAIL ADMIN|REVIEWER|OPERATOR|SUPPORT
//! audeniq-admin staff reset-mfa EMAIL   (lost authenticator: re-enroll at next sign-in)
//! audeniq-admin staff revoke EMAIL
//!   (AUDENIQ employees for /api/staff; the API can only read this)
//! audeniq-admin dsp list
//!   (the D-1..D-11 registry with each direct route's onboarding gaps)
//! audeniq-admin --operator STAFF_EMAIL payout list [REQUESTED|ORDERED|...]
//! audeniq-admin --operator STAFF_EMAIL payout order REQUEST_ID
//! audeniq-admin --operator STAFF_EMAIL payout approve ORDER_ID   (not the order's creator)
//! audeniq-admin --operator STAFF_EMAIL payout submit ORDER_ID BANK_TX_ID
//! audeniq-admin --operator STAFF_EMAIL payout result ORDER_ID settled|failed|returned|unknown [BANK_TX_ID]
//! audeniq-admin --operator STAFF_EMAIL payout cancel ORDER_ID    (before submit only)
//!   (ADMIN/OPERATOR staff; SETTLED posts the ledger debit, UNKNOWN is never resent)
//! audeniq-admin --operator STAFF_EMAIL royalty import ORG DSP START END CUR FILE
//! audeniq-admin --operator STAFF_EMAIL royalty match ORG REPORT
//! audeniq-admin --operator STAFF_EMAIL royalty receipt ORG REPORT BANK_REF AMOUNT CUR EVIDENCE_JSON
//! audeniq-admin --operator STAFF_EMAIL royalty show ORG REPORT
//! audeniq-admin --operator STAFF_EMAIL payout return ORDER BANK_REF AMOUNT EVIDENCE_JSON [--refund-fee]
use audeniq_core::protected_admin as admin;
use audeniq_core::protected_names::{Action, Mode};

const USAGE: &str = "usage: audeniq-admin [--operator NAME] protected <list|add|remove|activate|alias|remove-alias|grant-exception|revoke-exception> ...\n       audeniq-admin [--operator NAME] identifier-issuer <list|register UPC|ISRC PREFIX>\n       audeniq-admin [--operator NAME] partner <list|config-check|status ID|set-dsp ID [DSP_UUID]|probe ID|dpid ID DPID|test-ern ID FILE|test-ack ID FILE|capabilities ID JSON|contract ID REF|go-live ID|suspend ID --reason TEXT|route D-n DIRECT|MERLIN|merlin-eligible D-n true|false>\n       audeniq-admin [--operator NAME] staff <list|grant EMAIL ROLE|revoke EMAIL|reset-mfa EMAIL>\n       audeniq-admin dsp list\n       audeniq-admin --operator STAFF_EMAIL payout <list [STATUS]|order REQUEST_ID|approve ORDER_ID|submit ORDER_ID BANK_TX|result ORDER_ID settled|failed|returned|unknown [BANK_TX]|cancel ORDER_ID>\n       audeniq-admin --operator STAFF_EMAIL payout return ORDER_ID BANK_REF AMOUNT EVIDENCE_JSON_FILE [--refund-fee]\n       audeniq-admin --operator STAFF_EMAIL royalty <import ORG DSP START END CUR FILE|match ORG REPORT|receipt ORG REPORT BANK_REF AMOUNT CUR EVIDENCE_JSON_FILE|show ORG REPORT>";

fn take_opt(args: &mut Vec<String>, key: &str) -> Option<String> {
    let i = args.iter().position(|a| a == key)?;
    if i + 1 >= args.len() {
        return None;
    }
    let v = args.remove(i + 1);
    args.remove(i);
    Some(v)
}
fn take_flag(args: &mut Vec<String>, key: &str) -> bool {
    match args.iter().position(|a| a == key) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["upload-sandbox-check"] {
        audeniq_core::parser_sandbox::self_test()?;
        println!("upload parser isolation verified");
        return Ok(());
    }
    let operator = take_opt(&mut args, "--operator")
        .or_else(|| std::env::var("AUDENIQ_OPERATOR").ok())
        .unwrap_or_default();
    let mode = take_opt(&mut args, "--mode").map(|m| m.to_ascii_uppercase());
    let action = take_opt(&mut args, "--action").map(|a| a.to_ascii_uppercase());
    for (v, ok) in [
        (&mode, &["CONTAINS", "TOKEN"][..]),
        (&action, &["BLOCK", "REVIEW"][..]),
    ] {
        if let Some(v) = v
            && !ok.contains(&v.as_str())
        {
            anyhow::bail!("invalid value {v}; expected one of {ok:?}");
        }
    }
    let mode = Mode::parse(mode.as_deref().unwrap_or("CONTAINS"));
    let action = Action::parse(action.as_deref().unwrap_or("BLOCK"));
    let note = take_opt(&mut args, "--note");
    let reason = take_opt(&mut args, "--reason");
    let phrase = take_flag(&mut args, "--phrase");
    if args.first().map(String::as_str) == Some("privacy") {
        if operator.trim().is_empty() {
            anyhow::bail!("--operator NAME is required");
        }
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        let result = match args.get(1).map(String::as_str) {
            Some("purge-transient") if args.len() == 2 => {
                audeniq_core::privacy_maintenance::purge_transient(&pool, &operator).await?
            }
            Some("reencrypt-accounts") if args.len() == 2 => {
                audeniq_core::privacy_maintenance::reencrypt_accounts(&pool, &operator).await?
            }
            Some("encrypt-pii") if args.len() == 2 => {
                audeniq_core::privacy_maintenance::encrypt_pii(&pool, &operator).await?
            }
            _ => anyhow::bail!(
                "usage: audeniq-admin --operator NAME privacy <purge-transient|reencrypt-accounts|encrypt-pii>"
            ),
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("external-recording") {
        use audeniq_core::external_recordings;
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 2).await?;
        match (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
            (Some("list"), None, None) => {
                let rows: Vec<serde_json::Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'title',title,'artist',artist,'isrc',isrc,'source_url',source_url,'active',active,'imported_by',imported_by,'imported_at',imported_at) FROM catalog.external_recordings ORDER BY imported_at DESC LIMIT 1000")
                    .fetch_all(&pool).await?;
                println!("{}", serde_json::to_string_pretty(&rows)?);
            }
            (Some("import"), Some(metadata), Some(audio)) if args.len() == 4 => {
                let file = std::fs::File::open(metadata)?;
                if file.metadata()?.len() > 16384 {
                    anyhow::bail!("reference metadata exceeds 16 KiB");
                }
                let metadata: external_recordings::ReferenceMetadata =
                    serde_json::from_reader(file)?;
                let id = external_recordings::import(
                    &pool,
                    &operator,
                    metadata,
                    std::path::Path::new(audio),
                )
                .await?;
                println!("{id}");
            }
            (Some(cmd @ ("activate" | "deactivate")), Some(id), None) if args.len() == 3 => {
                external_recordings::set_active(
                    &pool,
                    &operator,
                    uuid::Uuid::parse_str(id)?,
                    cmd == "activate",
                )
                .await?;
                println!("ok");
            }
            _ => anyhow::bail!(
                "usage: audeniq-admin [--operator NAME] external-recording <list|import METADATA_JSON AUDIO_FILE|activate ID|deactivate ID>"
            ),
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("fingerprint") {
        let limit = take_opt(&mut args, "--limit")
            .map(|n| n.parse::<i64>())
            .transpose()?
            .unwrap_or(1000);
        if args.len() != 2 || args[1] != "backfill" {
            anyhow::bail!("usage: audeniq-admin fingerprint backfill [--limit N]");
        }
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        let queued = audeniq_core::submission::queue_fingerprint_backfill(&pool, limit).await?;
        println!("{}", serde_json::json!({"queued": queued}));
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("staff") {
        use audeniq_core::staff_admin;
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        match (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
            (Some("list"), None, None) => println!(
                "{}",
                serde_json::to_string_pretty(&staff_admin::list(&pool).await?)?
            ),
            (Some(cmd @ ("grant" | "revoke")), Some(email), role) => {
                if operator.trim().is_empty() {
                    anyhow::bail!("--operator NAME (or AUDENIQ_OPERATOR) is required for changes");
                }
                if cmd == "grant" {
                    let role = role.ok_or_else(|| anyhow::anyhow!(USAGE))?;
                    staff_admin::grant(&pool, &operator, email, &role.to_ascii_uppercase()).await?;
                } else {
                    staff_admin::revoke(&pool, &operator, email).await?;
                }
                println!("ok");
            }
            // Lost authenticator: the next sign-in enrolls a new one.
            (Some("reset-mfa"), Some(email), None) => {
                if operator.trim().is_empty() {
                    anyhow::bail!("--operator NAME (or AUDENIQ_OPERATOR) is required for changes");
                }
                let keys = audeniq_core::pii::cli_keys()?;
                let user = audeniq_core::pii::user_by_email(&keys, &pool, email)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("no such account"))?;
                audeniq_core::staff_mfa::reset(&pool, &operator, user).await?;
                println!("ok");
            }
            _ => anyhow::bail!(USAGE),
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("royalty") {
        use audeniq_core::royalty;
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        let actor: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT u.id FROM identity.users u JOIN identity.staff_members s ON s.user_id=u.id
             WHERE u.email=lower($1) AND u.status='ACTIVE' AND s.status='ACTIVE'
               AND s.role IN ('ADMIN','OPERATOR')",
        )
        .bind(operator.trim())
        .fetch_optional(&pool)
        .await?;
        let actor = actor.ok_or_else(|| anyhow::anyhow!("active ADMIN/OPERATOR email required"))?;
        let arg = |i: usize| -> anyhow::Result<&str> {
            args.get(i)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("royalty arguments missing"))
        };
        let org: uuid::Uuid = arg(2)?.parse()?;
        let output = match arg(1)? {
            "import" => {
                let content = std::fs::read_to_string(arg(7)?)?;
                let id = royalty::ingest_report(
                    &pool,
                    org,
                    arg(3)?,
                    arg(4)?.parse()?,
                    arg(5)?.parse()?,
                    arg(6)?,
                    arg(7)?,
                    &content,
                )
                .await?;
                serde_json::json!({"report_id":id,"status":"NORMALIZED"})
            }
            "match" => {
                let report: uuid::Uuid = arg(3)?.parse()?;
                let (matched, unresolved) = royalty::match_report(&pool, org, report).await?;
                serde_json::json!({"report_id":report,"newly_auto_matched":matched,"unresolved":unresolved})
            }
            "receipt" => {
                let evidence = serde_json::from_str(&std::fs::read_to_string(arg(7)?)?)?;
                let id = royalty::record_receipt(
                    &pool,
                    org,
                    arg(3)?.parse()?,
                    actor,
                    arg(4)?,
                    arg(5)?.parse()?,
                    arg(6)?,
                    evidence,
                )
                .await?;
                serde_json::json!({"receipt_id":id,"payable_posted":false})
            }
            "show" => {
                let value:Option<serde_json::Value>=sqlx::query_scalar(
                    "SELECT jsonb_build_object('report_id',r.id,'dsp_id',r.dsp_id,'status',r.status,
                       'currency',r.currency,'source_hash',r.source_hash,
                       'source_amount',(SELECT COALESCE(SUM(gross_amount),0)::text FROM finance.report_lines WHERE org_id=$1 AND report_id=r.id),
                       'received_amount',(SELECT COALESCE(SUM(amount),0)::text FROM finance.royalty_receipts WHERE org_id=$1 AND report_id=r.id),
                       'unresolved',(SELECT COUNT(*) FROM finance.report_lines WHERE org_id=$1 AND report_id=r.id AND match_status NOT IN ('AUTO','MANUAL')))
                     FROM finance.royalty_reports r WHERE r.org_id=$1 AND r.id=$2",
                ).bind(org).bind(arg(3)?.parse::<uuid::Uuid>()?).fetch_optional(&pool).await?;
                value.ok_or_else(|| anyhow::anyhow!("report not found"))?
            }
            _ => anyhow::bail!("royalty <import|match|receipt|show>"),
        };
        let mut tx = pool.begin().await?;
        audeniq_core::operations::audit(
            &mut tx,
            Some(actor),
            Some(org),
            None,
            &format!("royalty.{}", arg(1)?),
            "OWNER_CLI",
            uuid::Uuid::new_v4(),
        )
        .await?;
        tx.commit().await?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("payout") {
        use audeniq_core::finance;
        let refund_fee = take_flag(&mut args, "--refund-fee");
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        let uuid = |i: usize| -> anyhow::Result<uuid::Uuid> {
            Ok(args.get(i).ok_or_else(|| anyhow::anyhow!(USAGE))?.parse()?)
        };
        if args.get(1).map(String::as_str) == Some("list") {
            let status = args.get(2).cloned().unwrap_or_else(|| "REQUESTED".into());
            let mut rows: serde_json::Value = serde_json::Value::Array(sqlx::query_scalar(concat!(
                "SELECT jsonb_build_object('request_id',p.id,'org_id',p.org_id,'amount',p.amount::text,'fee',p.fee_amount::text,
                   'net',p.net_amount::text,'status',p.status,
                   'account',p.account_snapshot - 'holder_name' || jsonb_build_object('holder_name',",
                audeniq_core::pii_ref!(
                    "holder_name",
                    "p.org_id",
                    "p.account_snapshot->>'holder_name'",
                    "p.holder_cipher"
                ),
                "),'created_at',p.created_at,
                   'order_id',o.id,'order_status',o.status)
                 FROM portal.payout_requests p LEFT JOIN finance.payout_orders o ON o.id=p.payout_order_id
                 WHERE p.status=$1 ORDER BY p.created_at LIMIT 500"
            ))
            .bind(status.to_ascii_uppercase())
            .fetch_all(&pool)
            .await?);
            audeniq_core::pii::reveal(&audeniq_core::pii::cli_keys()?, &mut rows)?;
            println!("{}", serde_json::to_string_pretty(&rows)?);
            return Ok(());
        }
        // Every change is attributed to an active ADMIN/OPERATOR staff account.
        let keys = audeniq_core::pii::cli_keys()?;
        let operator_email = operator.trim().to_lowercase();
        let actor: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT u.id FROM identity.users u JOIN identity.staff_members s ON s.user_id=u.id
             WHERE (u.email=$1 OR u.email_bidx=ANY($2)) AND s.status='ACTIVE' AND s.role IN ('ADMIN','OPERATOR')",
        )
        .bind(&operator_email)
        .bind(audeniq_core::pii::email_indexes(&keys, &operator_email))
        .fetch_optional(&pool)
        .await?;
        let actor = actor.ok_or_else(|| {
            anyhow::anyhow!(
                "--operator must be the email of an active ADMIN/OPERATOR staff account"
            )
        })?;
        let org_of = |order: uuid::Uuid| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, uuid::Uuid>(
                    "SELECT org_id FROM finance.payout_orders WHERE id=$1",
                )
                .bind(order)
                .fetch_optional(&pool)
                .await?
                .ok_or_else(|| anyhow::anyhow!("payout order not found"))
            }
        };
        let out = match args.get(1).map(String::as_str) {
            Some("return") => {
                let id = uuid(2)?;
                let reference = args.get(3).ok_or_else(|| anyhow::anyhow!(USAGE))?;
                let amount = args.get(4).ok_or_else(|| anyhow::anyhow!(USAGE))?.parse()?;
                let evidence = serde_json::from_str(&std::fs::read_to_string(
                    args.get(5).ok_or_else(|| anyhow::anyhow!(USAGE))?,
                )?)?;
                let return_id = finance::record_payout_return(
                    &pool,
                    org_of(id).await?,
                    actor,
                    id,
                    finance::PayoutReturnInput {
                        bank_reference: reference,
                        returned_amount: amount,
                        refund_fee,
                        evidence,
                    },
                )
                .await?;
                serde_json::json!({"order_id":id,"return_id":return_id})
            }
            Some("order") => {
                let request = uuid(2)?;
                let org: uuid::Uuid =
                    sqlx::query_scalar("SELECT org_id FROM portal.payout_requests WHERE id=$1")
                        .bind(request)
                        .fetch_optional(&pool)
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("payout request not found"))?;
                let id = finance::order_payout_request(&pool, org, request, actor).await?;
                serde_json::json!({"order_id": id})
            }
            Some("approve") => {
                let id = uuid(2)?;
                finance::approve_payout_order(&pool, org_of(id).await?, actor, id).await?;
                serde_json::json!({"order_id": id, "status": "APPROVED"})
            }
            Some("submit") => {
                let id = uuid(2)?;
                let bank = args.get(3).ok_or_else(|| anyhow::anyhow!(USAGE))?;
                finance::mark_payout_submitted(&pool, org_of(id).await?, actor, id, bank).await?;
                serde_json::json!({"order_id": id, "status": "SUBMITTED"})
            }
            Some("result") => {
                let id = uuid(2)?;
                let outcome = match args.get(3).map(|s| s.to_ascii_lowercase()).as_deref() {
                    Some("settled") => finance::BankOutcome::Settled,
                    Some("failed") => finance::BankOutcome::Failed,
                    Some("returned") => finance::BankOutcome::Returned,
                    Some("unknown") => finance::BankOutcome::Unknown,
                    _ => anyhow::bail!(USAGE),
                };
                finance::record_bank_result(
                    &pool,
                    org_of(id).await?,
                    actor,
                    id,
                    args.get(4).map(String::as_str),
                    outcome,
                    serde_json::json!({"recorded_by": operator.trim()}),
                )
                .await?;
                serde_json::json!({"order_id": id, "status": finance::get_payout_status(&pool, org_of(id).await?, id).await?})
            }
            Some("cancel") => {
                let id = uuid(2)?;
                finance::cancel_payout_order(&pool, org_of(id).await?, id).await?;
                serde_json::json!({"order_id": id, "status": "CANCELLED"})
            }
            _ => anyhow::bail!(USAGE),
        };
        println!("{out}");
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("dsp") {
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        match args.get(1).map(String::as_str) {
            Some("list") => println!(
                "{}",
                serde_json::to_string_pretty(
                    &audeniq_core::staff_admin::dsp_overview(&pool).await?
                )?
            ),
            _ => anyhow::bail!(USAGE),
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("partner") {
        use audeniq_core::partner_admin as pa;
        use audeniq_core::partner_onboarding::{list_profiles, set_dsp};
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        match (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
            (Some("list"), None, None) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&list_profiles(&pool).await?)?
                )
            }
            (Some("set-dsp"), Some(partner), dsp) => {
                if operator.trim().is_empty() {
                    anyhow::bail!("--operator NAME (or AUDENIQ_OPERATOR) is required for changes");
                }
                let dsp = dsp.map(|d| uuid::Uuid::parse_str(d)).transpose()?;
                let id = set_dsp(&pool, &operator, partner, dsp).await?;
                println!("{partner} dsp_id={id}\nok");
            }
            (Some("config-check"), None, None) => {
                println!("{}", serde_json::to_string_pretty(&pa::check_configs())?)
            }
            (Some("status"), Some(partner), None) => println!(
                "{}",
                serde_json::to_string_pretty(
                    &audeniq_core::partner_onboarding::status(&pool, partner).await?
                )?
            ),
            (Some(cmd), Some(partner), arg) => {
                if operator.trim().is_empty() {
                    anyhow::bail!("--operator NAME (or AUDENIQ_OPERATOR) is required for changes");
                }
                let need = || arg.cloned().ok_or_else(|| anyhow::anyhow!(USAGE));
                let out: serde_json::Value = match cmd {
                    "probe" => pa::probe(&pool, &operator, partner).await?,
                    "dpid" => {
                        pa::set_recipient_dpid(&pool, &operator, partner, &need()?).await?;
                        serde_json::json!({"ok": true})
                    }
                    "test-ern" => {
                        let xml = std::fs::read_to_string(need()?)?;
                        pa::test_ern(&pool, &operator, partner, &xml).await?
                    }
                    "test-ack" => {
                        let bytes = std::fs::read(need()?)?;
                        pa::test_ack(&pool, &operator, partner, &bytes).await?
                    }
                    "capabilities" => {
                        let flags: serde_json::Value = serde_json::from_str(&need()?)?;
                        pa::set_capabilities(&pool, &operator, partner, &flags).await?
                    }
                    "contract" => {
                        pa::contract(&pool, &operator, partner, &need()?).await?;
                        serde_json::json!({"ok": true})
                    }
                    "go-live" => pa::go_live(&pool, &operator, partner).await?,
                    "route" => pa::set_route(&pool, &operator, partner, &need()?).await?,
                    "merlin-eligible" => {
                        let on = match need()?.as_str() {
                            "true" => true,
                            "false" => false,
                            _ => anyhow::bail!(USAGE),
                        };
                        pa::set_merlin_eligible(&pool, &operator, partner, on).await?;
                        serde_json::json!({"ok": true})
                    }
                    "suspend" => {
                        let why = reason.clone().unwrap_or_default();
                        pa::suspend(&pool, &operator, partner, &why).await?;
                        serde_json::json!({"ok": true})
                    }
                    _ => anyhow::bail!(USAGE),
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
            }
            _ => anyhow::bail!(USAGE),
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("identifier-issuer") {
        use audeniq_core::identifiers::{IdentifierKind, list_issuers, register_issuer};
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        match (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
            (Some("list"), None, None) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&list_issuers(&pool).await?)?
                )
            }
            (Some("register"), Some(kind), Some(prefix)) => {
                if operator.trim().is_empty() {
                    anyhow::bail!("--operator NAME (or AUDENIQ_OPERATOR) is required for changes");
                }
                let id =
                    register_issuer(&pool, &operator, IdentifierKind::parse(kind)?, prefix).await?;
                println!("registered {id}\nok");
            }
            _ => anyhow::bail!(USAGE),
        }
        return Ok(());
    }
    if args.first().map(String::as_str) != Some("protected") || args.len() < 2 {
        anyhow::bail!(USAGE);
    }
    let cmd = args[1].clone();
    let rest: Vec<String> = args[2..].to_vec();
    let need_operator = || -> anyhow::Result<&str> {
        if operator.trim().is_empty() {
            anyhow::bail!("--operator NAME (or AUDENIQ_OPERATOR) is required for changes");
        }
        Ok(operator.as_str())
    };
    let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
    let arg = |i: usize| -> anyhow::Result<&str> {
        rest.get(i)
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!(USAGE))
    };
    match cmd.as_str() {
        "list" => println!(
            "{}",
            serde_json::to_string_pretty(&admin::list(&pool).await?)?
        ),
        "add" => {
            let id = admin::add(
                &pool,
                need_operator()?,
                arg(0)?,
                mode,
                action,
                note.as_deref(),
            )
            .await?;
            println!("added {id}");
        }
        "remove" => admin::set_active(&pool, need_operator()?, arg(0)?, false).await?,
        "activate" => admin::set_active(&pool, need_operator()?, arg(0)?, true).await?,
        "alias" => {
            admin::add_alias(
                &pool,
                need_operator()?,
                arg(0)?,
                arg(1)?,
                phrase,
                mode,
                action,
            )
            .await?
        }
        "remove-alias" => admin::remove_alias(&pool, need_operator()?, arg(0)?, arg(1)?).await?,
        "grant-exception" => {
            let org = uuid::Uuid::parse_str(arg(1)?)?;
            let reason = reason.ok_or_else(|| anyhow::anyhow!("--reason TEXT is required"))?;
            admin::grant_exception(&pool, need_operator()?, arg(0)?, org, &reason).await?
        }
        "revoke-exception" => {
            let org = uuid::Uuid::parse_str(arg(1)?)?;
            admin::revoke_exception(&pool, need_operator()?, arg(0)?, org).await?
        }
        _ => anyhow::bail!(USAGE),
    }
    if cmd != "list" {
        println!("ok");
    }
    Ok(())
}
