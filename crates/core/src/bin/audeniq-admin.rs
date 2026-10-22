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
//! audeniq-admin staff grant EMAIL OWNER|ADMIN|REVIEWER|OPERATOR|SUPPORT
//!   (levels: OWNER 4 > ADMIN 3 > REVIEWER, OPERATOR 2 > SUPPORT 1; OWNER only
//!    here — the admin console grants the levels below an OWNER's)
//! audeniq-admin staff reset-mfa EMAIL   (lost authenticator: re-enroll at next sign-in)
//! audeniq-admin staff revoke EMAIL
//!   (AUDENIQ employees for /api/staff; the console changes staff only through
//!    identity.staff_set, never an OWNER or oneself)
//!
//! Most of the commands below also run in the admin console (crate::staff_ops)
//! when AUDENIQ_STAFF_OPS_DATABASE_URL is set; deployment commands stay here.
//! audeniq-admin upload-sandbox-check   (deploy check: the upload parser sandbox holds)
//! audeniq-admin antivirus-update       (FreshClam behind the TLS-only proxy; the antivirus-update service)
//! audeniq-admin dsp list
//! audeniq-admin mcp-client list
//! audeniq-admin --operator NAME mcp-client add NAME [--password-grant] [--secret-post] [--redirect-uri URI]...
//!   (an AI agent that authenticates with a client secret; --password-grant only for agents whose
//!    credential form sends the e-mail and password to AUDENIQ without the model seeing them.
//!    Prints the client id and secret once.)
//! audeniq-admin --operator NAME mcp-client disable CLIENT_ID   (ends all of its connections)
//! audeniq-admin developer-app list [PRIVATE|REVIEW|PUBLIC]   (apps users registered for the REST API, review queue first)
//! audeniq-admin --operator NAME developer-app approve CLIENT_ID [--note TEXT]   (any artist can connect it)
//! audeniq-admin --operator NAME developer-app return CLIENT_ID --note TEXT   (back to its owner only; other artists' connections end)
//! audeniq-admin mcp-limit list
//! audeniq-admin --operator NAME mcp-limit set EMAIL N [--note TEXT]
//! audeniq-admin --operator NAME mcp-limit clear EMAIL
//!   (a user's daily limit for the public AI tools after a support inquiry; clear = default)
//!   (the D-1..D-11 registry with each direct route's onboarding gaps)
//! audeniq-admin --operator STAFF_EMAIL payout list [REQUESTED|ORDERED|...]
//! audeniq-admin --operator STAFF_EMAIL payout order REQUEST_ID
//! audeniq-admin --operator STAFF_EMAIL payout approve ORDER_ID   (not the order's creator)
//! audeniq-admin --operator STAFF_EMAIL payout submit ORDER_ID BANK_TX_ID
//! audeniq-admin --operator STAFF_EMAIL payout result ORDER_ID settled|failed|returned|unknown [BANK_TX_ID]
//! audeniq-admin --operator STAFF_EMAIL payout cancel ORDER_ID    (before submit only)
//! audeniq-admin --operator STAFF_EMAIL payout manual ORDER_ID BANK_TX EVIDENCE_JSON_FILE
//!     (paid in the bank's own channel; transfer evidence required, executor ≠ approver)
//! audeniq-admin --operator STAFF_EMAIL payout reissue ORDER_ID   (after RETURNED/FAILED)
//!   (OWNER/ADMIN/OPERATOR staff; SETTLED posts the ledger debit, UNKNOWN is never resent)
//! audeniq-admin --operator STAFF_EMAIL royalty import ORG DSP START END CUR FILE
//! audeniq-admin --operator STAFF_EMAIL royalty match ORG REPORT
//! audeniq-admin --operator STAFF_EMAIL royalty receipt ORG REPORT BANK_REF AMOUNT CUR EVIDENCE_JSON
//! audeniq-admin --operator STAFF_EMAIL royalty post ORG REPORT FX_RATE
//!   (fully matched + paid report → ledger: fee, applicant, collaborator splits)
//! audeniq-admin --operator STAFF_EMAIL royalty show ORG REPORT
//! audeniq-admin --operator STAFF_EMAIL payout return ORDER BANK_REF AMOUNT EVIDENCE_JSON [--refund-fee]
//! audeniq-admin storage probe [--origin URL] [--archive]   (bucket check: docs/STORAGE_PROVIDERS.md)
//! audeniq-admin maintenance status
//! audeniq-admin --operator NAME maintenance start [--minutes N] [--title TEXT] [--message TEXT]
//!   (emergency maintenance from now; no --minutes = end unknown)
//! audeniq-admin --operator NAME maintenance extend --minutes N
//! audeniq-admin --operator NAME maintenance stop
//! audeniq-admin maintenance list
//! audeniq-admin --operator NAME maintenance schedule --from TIME --to TIME [--announce-from TIME] [--title TEXT] [--message TEXT]
//! audeniq-admin --operator NAME maintenance cancel ID   (a planned one; a running one is `stop`)
//! audeniq-admin notice list
//! audeniq-admin --operator NAME notice post --title TEXT (--body TEXT | --body-file FILE) [--pin] [--at TIME]
//! audeniq-admin --operator NAME notice remove ID
//!   (emergency work only, straight in the database, so it works while the
//!    API, sign-in and the admin console are down; the API applies it within
//!    seconds. TIME: KST `YYYY-MM-DD HH:MM` or UTC `YYYY-MM-DDTHH:MM:SSZ`)
use audeniq_core::protected_admin as admin;
use audeniq_core::protected_names::{Action, Mode};

const USAGE: &str = "usage: audeniq-admin [--operator NAME] protected <list|add|remove|activate|alias|remove-alias|grant-exception|revoke-exception> ...\n       audeniq-admin [--operator NAME] identifier-issuer <list|register UPC|ISRC PREFIX>\n       audeniq-admin [--operator NAME] partner <list|config-check|status ID|set-dsp ID [DSP_UUID]|probe ID|dpid ID DPID|test-ern ID FILE|test-ack ID FILE|capabilities ID JSON|contract ID REF|go-live ID|suspend ID --reason TEXT|route D-n DIRECT|MERLIN|merlin-eligible D-n true|false>\n       audeniq-admin [--operator NAME] staff <list|grant EMAIL ROLE|revoke EMAIL|reset-mfa EMAIL>\n       audeniq-admin dsp list\n       audeniq-admin [--operator NAME] mcp-client <list|add NAME [--password-grant] [--secret-post] [--redirect-uri URI]...|disable CLIENT_ID>\n       audeniq-admin [--operator NAME] developer-app <list [STATUS]|approve CLIENT_ID [--note TEXT]|return CLIENT_ID --note TEXT>\n       audeniq-admin [--operator NAME] mcp-limit <list|set EMAIL N [--note TEXT]|clear EMAIL>\n       audeniq-admin --operator STAFF_EMAIL payout <list [STATUS]|order REQUEST_ID|approve ORDER_ID|submit ORDER_ID BANK_TX|result ORDER_ID settled|failed|returned|unknown [BANK_TX]|manual ORDER_ID BANK_TX EVIDENCE_JSON_FILE|reissue ORDER_ID|cancel ORDER_ID>\n       audeniq-admin --operator STAFF_EMAIL payout return ORDER_ID BANK_REF AMOUNT EVIDENCE_JSON_FILE [--refund-fee]\n       audeniq-admin --operator STAFF_EMAIL royalty <import ORG DSP START END CUR FILE|match ORG REPORT|receipt ORG REPORT BANK_REF AMOUNT CUR EVIDENCE_JSON_FILE|post ORG REPORT FX_RATE|show ORG REPORT>";

const MAINTENANCE_USAGE: &str = "usage: audeniq-admin maintenance <status|list>\n       audeniq-admin --operator NAME maintenance start [--minutes N] [--title TEXT] [--message TEXT]\n       audeniq-admin --operator NAME maintenance extend --minutes N\n       audeniq-admin --operator NAME maintenance stop\n       audeniq-admin --operator NAME maintenance schedule --from TIME --to TIME [--announce-from TIME] [--title TEXT] [--message TEXT]\n       audeniq-admin --operator NAME maintenance cancel ID\n  TIME: KST 'YYYY-MM-DD HH:MM' or UTC 'YYYY-MM-DDTHH:MM:SSZ'";
const NOTICE_USAGE: &str = "usage: audeniq-admin notice list\n       audeniq-admin --operator NAME notice post --title TEXT (--body TEXT | --body-file FILE) [--pin] [--at TIME]\n       audeniq-admin --operator NAME notice remove ID";

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

fn main() -> anyhow::Result<()> {
    // Plain processes, before any async runtime or thread starts.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        // The child that `upload-sandbox-check` runs inside the sandbox.
        [audeniq_core::parser_sandbox::PROBE_ARG] => {
            if let Err(e) = audeniq_core::parser_sandbox::probe() {
                eprintln!("{e}");
                std::process::exit(1);
            }
            return Ok(());
        }
        ["upload-sandbox-check"] => {
            audeniq_core::parser_sandbox::self_test()?;
            println!("upload parser isolation verified");
            return Ok(());
        }
        // The antivirus-update service: FreshClam behind the TLS-only proxy.
        ["antivirus-update"] => {
            let status = audeniq_core::av_update::run()?;
            std::process::exit(status.code().unwrap_or(1));
        }
        _ => {}
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(args))
}

async fn run(mut args: Vec<String>) -> anyhow::Result<()> {
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
    // Server maintenance straight in the database: works while the API, sign-in
    // and the admin console are unreachable. A running API applies it within
    // seconds (site_content::STATUS_TTL).
    if args.first().map(String::as_str) == Some("maintenance") {
        use audeniq_core::site_content;
        let minutes = take_opt(&mut args, "--minutes")
            .map(|m| m.parse::<i64>())
            .transpose()
            .map_err(|_| anyhow::anyhow!("--minutes must be a number"))?;
        let title = take_opt(&mut args, "--title");
        let message = take_opt(&mut args, "--message");
        let from = take_opt(&mut args, "--from");
        let to = take_opt(&mut args, "--to");
        let announce = take_opt(&mut args, "--announce-from");
        let sub = args.get(1).map(String::as_str);
        if !matches!(sub, Some("status" | "list")) && operator.trim().is_empty() {
            anyhow::bail!("--operator NAME (or AUDENIQ_OPERATOR) is required for changes");
        }
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        let out = match (sub, args.len()) {
            (Some("status"), 2) => site_content::cli_status(&pool).await?,
            (Some("list"), 2) => site_content::cli_list(&pool, "maintenance").await?,
            // Planned maintenance: shown as a notice banner from --announce-from
            // (default now), blocking the API from --from to --to.
            (Some("schedule"), 2) => {
                let (Some(from), Some(to)) = (from, to) else {
                    anyhow::bail!("schedule needs --from TIME --to TIME");
                };
                let mut input = serde_json::json!({
                    "title": title.as_deref().unwrap_or("서버 점검"),
                    "body": message.unwrap_or_default(),
                    "starts_at": site_content::cli_time(&from)?,
                    "ends_at": site_content::cli_time(&to)?,
                    "kind": "scheduled",
                });
                if let Some(a) = announce {
                    input["published_at"] = site_content::cli_time(&a)?.into();
                }
                site_content::cli_create(&pool, &operator, "maintenance", input).await?
            }
            (Some("cancel"), 3) => {
                site_content::cli_remove(&pool, &operator, "maintenance", &args[2]).await?
            }
            (Some("start"), 2) => {
                site_content::cli_start(
                    &pool,
                    &operator,
                    minutes,
                    title.as_deref(),
                    message.as_deref(),
                )
                .await?
            }
            (Some("stop"), 2) => site_content::cli_stop(&pool, &operator).await?,
            (Some("extend"), 2) => {
                let m = minutes.ok_or_else(|| anyhow::anyhow!("extend needs --minutes N"))?;
                site_content::cli_extend(&pool, &operator, m).await?
            }
            _ => anyhow::bail!(MAINTENANCE_USAGE),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        if std::env::var("MAINTENANCE_MODE").is_ok_and(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "" | "0" | "off" | "false" | "no"
            )
        }) {
            eprintln!(
                "MAINTENANCE_MODE is on in this environment: that emergency switch keeps the API blocked until it is turned off and the API restarted."
            );
        }
        return Ok(());
    }
    // Emergency notices, same rows as the admin console's notices (readable
    // during maintenance: /api/notices stays open).
    if args.first().map(String::as_str) == Some("notice") {
        use audeniq_core::site_content;
        let title = take_opt(&mut args, "--title");
        let body = take_opt(&mut args, "--body");
        let body_file = take_opt(&mut args, "--body-file");
        let at = take_opt(&mut args, "--at");
        let pin = take_flag(&mut args, "--pin");
        let sub = args.get(1).map(String::as_str);
        if sub != Some("list") && operator.trim().is_empty() {
            anyhow::bail!("--operator NAME (or AUDENIQ_OPERATOR) is required for changes");
        }
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        let out = match (sub, args.len()) {
            (Some("list"), 2) => site_content::cli_list(&pool, "notices").await?,
            (Some("post"), 2) => {
                let title = title.ok_or_else(|| anyhow::anyhow!("post needs --title TEXT"))?;
                let body = match (body, body_file) {
                    (Some(b), None) => b,
                    (None, Some(f)) => std::fs::read_to_string(f)?,
                    _ => anyhow::bail!("post needs --body TEXT or --body-file FILE"),
                };
                let mut input = serde_json::json!({"title": title, "body": body, "pinned": pin});
                if let Some(at) = at {
                    input["published_at"] = site_content::cli_time(&at)?.into();
                }
                site_content::cli_create(&pool, &operator, "notices", input).await?
            }
            (Some("remove"), 3) => {
                site_content::cli_remove(&pool, &operator, "notices", &args[2]).await?
            }
            _ => anyhow::bail!(NOTICE_USAGE),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
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
    // Checks a bucket really does what uploads / the archive rely on
    // (AWS S3, Cloudflare R2, Oracle Cloud): docs/STORAGE_PROVIDERS.md.
    if args.first().map(String::as_str) == Some("storage") {
        let archive = take_flag(&mut args, "--archive");
        let origin = take_opt(&mut args, "--origin")
            .or_else(|| std::env::var("APP_ORIGIN").ok())
            .filter(|o| !o.trim().is_empty());
        anyhow::ensure!(
            args.get(1).map(String::as_str) == Some("probe") && args.len() == 2,
            "usage: audeniq-admin storage probe [--origin https://studio.example] [--archive]"
        );
        let allow_http = std::env::var("ALLOW_HTTP_STORAGE").as_deref() == Ok("true");
        let store = audeniq_core::storage::s3_from_env(archive, allow_http)?;
        let r = if archive {
            store.probe_archive().await
        } else {
            store.probe(origin.as_deref()).await
        };
        println!("{}", serde_json::to_string_pretty(&r)?);
        if r["ok"] != true {
            std::process::exit(2);
        }
        return Ok(());
    }
    // Document integrity and the original-document archive (docs/DOCUMENT_ARCHIVE.md).
    if matches!(args.first().map(String::as_str), Some("seal" | "archive")) {
        if operator.trim().is_empty() {
            anyhow::bail!("--operator NAME is required");
        }
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 2).await?;
        let keys = audeniq_core::pii::cli_keys()?;
        let deep = take_flag(&mut args, "--deep");
        let out = take_opt(&mut args, "--out");
        let retain_days = take_opt(&mut args, "--retain-days")
            .map(|v| v.parse::<i64>())
            .transpose()?
            .unwrap_or(0);
        if args.first().map(String::as_str) == Some("seal") {
            anyhow::ensure!(
                args.get(1).map(String::as_str) == Some("verify"),
                "usage: audeniq-admin --operator NAME seal verify"
            );
            let mut c = pool.acquire().await?;
            let r = audeniq_core::seal::verify(&mut c, &keys).await?.json();
            println!("{}", serde_json::to_string_pretty(&r)?);
            if r["ok"] != true {
                std::process::exit(2);
            }
            return Ok(());
        }
        let allow_http = std::env::var("ALLOW_HTTP_STORAGE").as_deref() == Ok("true");
        let target =
            audeniq_core::storage::archive_store_from_env(allow_http)?.ok_or_else(|| {
                anyhow::anyhow!("ARCHIVE_S3_BUCKET is not set (docs/DOCUMENT_ARCHIVE.md)")
            })?;
        let result = match (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
            (Some("run"), period, None) => {
                let source = audeniq_core::storage::store_from_env(allow_http)?;
                let period = period.cloned().unwrap_or_else(|| {
                    audeniq_core::archive::previous_month(chrono::Utc::now().date_naive())
                });
                audeniq_core::archive::run(
                    &pool,
                    source.as_ref(),
                    target.as_ref(),
                    &keys,
                    &period,
                    &operator,
                )
                .await?
            }
            (Some("anchor"), None, None) => {
                audeniq_core::archive::anchor(&pool, target.as_ref(), &keys, &operator).await?
            }
            (Some("verify"), None, None) => {
                let r = audeniq_core::archive::verify(&pool, target.as_ref(), &keys, deep).await?;
                println!("{}", serde_json::to_string_pretty(&r)?);
                if r["ok"] != true {
                    std::process::exit(2);
                }
                return Ok(());
            }
            (Some("status"), None, None) => audeniq_core::archive::status(&pool).await?,
            (Some("open"), Some(key), None) => {
                let bytes =
                    audeniq_core::archive::open(&pool, target.as_ref(), &keys, &operator, key)
                        .await?;
                match out {
                    Some(path) => {
                        std::fs::write(&path, &bytes)?;
                        serde_json::json!({"written": path, "bytes": bytes.len()})
                    }
                    None => {
                        use std::io::Write;
                        std::io::stdout().write_all(&bytes)?;
                        return Ok(());
                    }
                }
            }
            (Some("put-backup"), Some(file), Some(key)) => {
                audeniq_core::archive::put_backup(
                    target.as_ref(),
                    std::path::Path::new(file),
                    key,
                    retain_days,
                )
                .await?
            }
            (Some("purge-expired"), None, None) => {
                audeniq_core::archive::purge_expired(&pool, target.as_ref(), &operator).await?
            }
            _ => anyhow::bail!(
                "usage: audeniq-admin --operator NAME archive <run [YYYY-MM]|anchor|verify [--deep]|status|open KEY [--out FILE]|put-backup FILE pitr/...|backups/... [--retain-days N]|purge-expired>\n       audeniq-admin --operator NAME seal verify"
            ),
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
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
    if args.first().map(String::as_str) == Some("mcp-client") {
        use audeniq_core::mcp::login;
        let password_grant = take_flag(&mut args, "--password-grant");
        let secret_post = take_flag(&mut args, "--secret-post");
        let mut redirects = Vec::new();
        while let Some(r) = take_opt(&mut args, "--redirect-uri") {
            redirects.push(r);
        }
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        match (args.get(1).map(String::as_str), args.get(2), args.len()) {
            (Some("list"), None, 2) => println!(
                "{}",
                serde_json::to_string_pretty(&login::admin_list_clients(&pool).await?)?
            ),
            (Some("add"), Some(name), 3) => {
                let (client_id, secret) = login::admin_add_client(
                    &pool,
                    &operator,
                    name,
                    password_grant,
                    secret_post,
                    &redirects,
                )
                .await?;
                println!(
                    "{}",
                    serde_json::json!({"client_id": client_id, "client_secret": secret, "password_grant": password_grant})
                );
                eprintln!(
                    "The client secret is shown only now. Give it to the agent over a secure channel."
                );
            }
            (Some("disable"), Some(id), 3) => {
                login::admin_disable_client(&pool, &operator, id).await?;
                println!("ok");
            }
            _ => anyhow::bail!(USAGE),
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("developer-app") {
        use audeniq_core::mcp::developer;
        let note = take_opt(&mut args, "--note");
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        match (args.get(1).map(String::as_str), args.get(2), args.len()) {
            (Some("list"), status, 2 | 3) => {
                let status = status.map(|v| v.to_ascii_uppercase());
                if let Some(v) = &status {
                    anyhow::ensure!(
                        matches!(v.as_str(), "PRIVATE" | "REVIEW" | "PUBLIC"),
                        "STATUS is PRIVATE, REVIEW or PUBLIC"
                    );
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &developer::admin_list(&pool, status.as_deref()).await?
                    )?
                );
            }
            (Some(action @ ("approve" | "return")), Some(id), 3) => {
                anyhow::ensure!(
                    action == "approve" || note.is_some(),
                    "returning an app needs --note TEXT for its owner"
                );
                let to =
                    developer::review(&pool, &operator, id, action == "approve", note.as_deref())
                        .await?;
                println!("{}", serde_json::json!({"client_id": id, "visibility": to}));
            }
            _ => anyhow::bail!(USAGE),
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("mcp-limit") {
        use audeniq_core::mcp::public;
        let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
        let user = |email: String| {
            let pool = pool.clone();
            async move {
                let keys = audeniq_core::pii::cli_keys()?;
                audeniq_core::pii::user_by_email(&keys, &pool, &email)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("no such account"))
            }
        };
        match (
            args.get(1).map(String::as_str),
            args.get(2),
            args.get(3),
            args.len(),
        ) {
            (Some("list"), None, None, 2) => println!(
                "{}",
                serde_json::to_string_pretty(&public::admin_list_limits(&pool).await?)?
            ),
            (Some("set"), Some(email), Some(n), 4) => {
                let n: i32 = n
                    .parse()
                    .map_err(|_| anyhow::anyhow!("N must be a number"))?;
                let id = user(email.clone()).await?;
                public::admin_set_limit(&pool, &operator, id, Some(n), note.as_deref()).await?;
                println!("ok");
            }
            (Some("clear"), Some(email), None, 3) => {
                let id = user(email.clone()).await?;
                public::admin_set_limit(&pool, &operator, id, None, None).await?;
                println!("ok");
            }
            _ => anyhow::bail!(USAGE),
        }
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
               AND s.role IN ('OWNER','ADMIN','OPERATOR')",
        )
        .bind(operator.trim())
        .fetch_optional(&pool)
        .await?;
        let actor =
            actor.ok_or_else(|| anyhow::anyhow!("active OWNER/ADMIN/OPERATOR email required"))?;
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
            "show" => audeniq_core::staff_ops::royalty_show(&pool, org, arg(3)?.parse()?).await?,
            "post" => {
                // KRW per unit of the report currency (1 for a KRW report).
                let fx: rust_decimal::Decimal = arg(4)?.parse()?;
                audeniq_core::royalty_posting::post_report(&pool, org, arg(3)?.parse()?, actor, fx)
                    .await?
            }
            _ => anyhow::bail!("royalty <import|match|receipt|post|show>"),
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
            let rows = audeniq_core::staff_ops::payout_list(
                &pool,
                &audeniq_core::pii::cli_keys()?,
                &status,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&rows)?);
            return Ok(());
        }
        // Every change is attributed to an active ADMIN/OPERATOR staff account.
        let keys = audeniq_core::pii::cli_keys()?;
        let operator_email = operator.trim().to_lowercase();
        let actor: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT u.id FROM identity.users u JOIN identity.staff_members s ON s.user_id=u.id
             WHERE (u.email=$1 OR u.email_bidx=ANY($2)) AND s.status='ACTIVE' AND s.role IN ('OWNER','ADMIN','OPERATOR')",
        )
        .bind(&operator_email)
        .bind(audeniq_core::pii::email_indexes(&keys, &operator_email))
        .fetch_optional(&pool)
        .await?;
        let actor = actor.ok_or_else(|| {
            anyhow::anyhow!(
                "--operator must be the email of an active OWNER/ADMIN/OPERATOR staff account"
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
            Some("manual") => {
                // Paid by hand in the bank's own channel (no transfer API
                // contract yet): evidence of the transfer is required.
                let id = uuid(2)?;
                let bank = args.get(3).ok_or_else(|| anyhow::anyhow!(USAGE))?;
                let evidence = serde_json::from_str(&std::fs::read_to_string(
                    args.get(4).ok_or_else(|| anyhow::anyhow!(USAGE))?,
                )?)?;
                finance::record_manual_payout(&pool, org_of(id).await?, actor, id, bank, evidence)
                    .await?;
                serde_json::json!({"order_id": id, "status": "SETTLED", "manual": true})
            }
            Some("reissue") => {
                let id = uuid(2)?;
                let new = finance::reissue_payout(&pool, org_of(id).await?, actor, id).await?;
                serde_json::json!({"reissue_of": id, "order_id": new, "status": "PENDING_APPROVAL"})
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
