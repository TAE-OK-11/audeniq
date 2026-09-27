//! Operator CLI. Runs with the schema-owner DATABASE_URL (never a runtime
//! role). Every change is logged with the operator name
//! (`--operator NAME` or `AUDENIQ_OPERATOR`).
//!
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
//! audeniq-admin staff list
//! audeniq-admin staff grant EMAIL ADMIN|REVIEWER|OPERATOR|SUPPORT
//! audeniq-admin staff revoke EMAIL
//!   (AUDENIQ employees for /api/staff; the API can only read this)
//! audeniq-admin dsp list
//!   (the D-1..D-11 registry with each direct route's onboarding gaps)
use audeniq_core::protected_admin as admin;
use audeniq_core::protected_names::{Action, Mode};

const USAGE: &str = "usage: audeniq-admin [--operator NAME] protected <list|add|remove|activate|alias|remove-alias|grant-exception|revoke-exception> ...\n       audeniq-admin [--operator NAME] identifier-issuer <list|register UPC|ISRC PREFIX>\n       audeniq-admin [--operator NAME] partner <list|set-dsp PARTNER_ID [DSP_UUID]>\n       audeniq-admin [--operator NAME] staff <list|grant EMAIL ROLE|revoke EMAIL>\n       audeniq-admin dsp list";

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
            _ => anyhow::bail!(USAGE),
        }
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
