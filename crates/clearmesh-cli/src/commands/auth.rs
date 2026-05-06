use crate::api::ApiClient;
use crate::config;
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Args)]
pub struct AuthCommand {
    #[command(subcommand)]
    pub command: AuthSubcommand,
}

#[derive(Subcommand)]
pub enum AuthSubcommand {
    Me,
    Security,
}

#[derive(Args)]
pub struct ConfigCommand {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Subcommand)]
pub enum ConfigSubcommand {
    Show,
    SetApi { url: String },
}

pub async fn login() -> Result<()> {
    let mut cfg = config::load()?;
    let api = ApiClient::from_config(&cfg);
    let start = api.post("/v1/auth/device/start", &json!({})).await?;
    let device_code = start["device_code"]
        .as_str()
        .context("device login response missing device_code")?
        .to_string();
    let user_code = start["user_code"]
        .as_str()
        .context("device login response missing user_code")?;
    let verification_uri = start["verification_uri"]
        .as_str()
        .context("device login response missing verification_uri")?;
    let verification_uri_complete = start["verification_uri_complete"]
        .as_str()
        .unwrap_or(verification_uri);
    let expires_in = start["expires_in"].as_u64().unwrap_or(900);
    let interval = start["interval"].as_u64().unwrap_or(5).max(2);

    println!("To authenticate ClearMesh CLI, open this URL in your browser:");
    println!();
    println!("  {verification_uri_complete}");
    println!();
    println!("Then confirm this code:");
    println!();
    println!("  {user_code}");
    println!();
    println!("Waiting for browser approval...");

    let deadline = Instant::now() + Duration::from_secs(expires_in);
    let value = loop {
        if Instant::now() >= deadline {
            bail!("device login expired; run clearmesh login again");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        match api
            .post(
                "/v1/auth/device/poll",
                &json!({ "device_code": device_code }),
            )
            .await
        {
            Ok(value) => {
                if value["status"].as_str() == Some("authorization_pending") {
                    continue;
                }
                break value;
            }
            Err(err) => {
                let message = err.to_string();
                if message.contains("authorization_pending") {
                    continue;
                }
                return Err(err);
            }
        }
    };

    cfg.token = value["token"].as_str().map(ToOwned::to_owned);
    cfg.email = value["user"]["email"].as_str().map(ToOwned::to_owned);
    cfg.name = value["user"]["name"].as_str().map(ToOwned::to_owned);
    config::save(&cfg)?;
    println!("Logged in as {}", cfg.email.as_deref().unwrap_or("unknown"));
    Ok(())
}

pub async fn logout() -> Result<()> {
    let mut cfg = config::load()?;
    if cfg.token.is_some() {
        let api = ApiClient::from_config(&cfg);
        let _ = api.post("/v1/auth/logout", &json!({})).await;
    }
    cfg.token = None;
    config::save(&cfg)?;
    println!("Logged out");
    Ok(())
}

pub async fn run_auth(command: AuthCommand) -> Result<()> {
    let cfg = config::load()?;
    let api = ApiClient::from_config(&cfg);

    match command.command {
        AuthSubcommand::Me => {
            let value = api.get("/v1/auth/me").await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        AuthSubcommand::Security => {
            let value = api.get("/v1/auth/security").await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }
    Ok(())
}

pub async fn run_config(command: ConfigCommand) -> Result<()> {
    match command.command {
        ConfigSubcommand::Show => {
            let cfg = config::load()?;
            println!("{}", serde_json::to_string_pretty(&cfg)?);
            Ok(())
        }
        ConfigSubcommand::SetApi { url } => {
            let mut cfg = config::load()?;
            cfg.api = url;
            config::save(&cfg)?;
            println!("API set to {}", cfg.api);
            Ok(())
        }
    }
}
