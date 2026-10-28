//! Offline trusted producer; no HTTP endpoint and no upload/raw-file input mode.
use audeniq_core::{
    api::AppState,
    config::Config,
    database,
    image_encoder::{self, WebpOptions},
    storage,
};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Internal mode is invoked only by the sandboxed parent, with cleared env.
    if args.first().map(String::as_str) == Some("sandbox-webp") {
        anyhow::ensure!(args.len() == 5, "invalid sandbox arguments");
        return image_encoder::sandbox_webp(
            Path::new(&args[1]),
            Path::new(&args[2]),
            &args[3],
            WebpOptions {
                max_edge: args[4].parse()?,
            },
        );
    }
    anyhow::ensure!(
        (3..=5).contains(&args.len()),
        "usage: audeniq-image-encode ORG_UUID ASSET_UUID SOURCE_SHA256 [MAX_EDGE=8192] [QUALITY=lossless]"
    );
    anyhow::ensure!(
        args.get(4).is_none_or(|v| v == "lossless"),
        "only lossless quality is supported"
    );
    let org = args[0].parse()?;
    let asset = args[1].parse()?;
    let options = WebpOptions {
        max_edge: args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(8192),
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let config = Config::from_env()?;
            let pool = database::connect(&config.database_url, 2).await?;
            let store =
                storage::store_from_env(std::env::var("APP_ENV").as_deref() != Ok("production"))?;
            let state = AppState::new(pool, config, store).await?;
            let digest = image_encoder::produce_webp(
                &state,
                org,
                asset,
                &args[2],
                options,
                &std::env::current_exe()?,
            )
            .await?;
            println!("registered image/webp sha256={digest}");
            Ok(())
        })
}
