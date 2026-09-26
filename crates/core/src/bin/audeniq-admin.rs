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
use audeniq_core::protected_admin as admin;
use audeniq_core::protected_names::{Action, Mode};

const USAGE: &str = "usage: audeniq-admin [--operator NAME] protected <list|add|remove|activate|alias|remove-alias|grant-exception|revoke-exception> ...";

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
