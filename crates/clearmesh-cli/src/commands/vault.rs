use crate::api::ApiClient;
use crate::commands::commit::require_local_repo;
use crate::config;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct VaultCommand {
    #[command(subcommand)]
    pub command: VaultSubcommand,
}

#[derive(Subcommand)]
pub enum VaultSubcommand {
    Status,
}

pub async fn run(command: VaultCommand) -> Result<()> {
    match command.command {
        VaultSubcommand::Status => {
            let (_, local) = require_local_repo()?;
            let cfg = config::load()?;
            let api = ApiClient::from_config(&cfg);
            let org = local.org.context("local repo has no org")?;
            let repo = local.repo.context("local repo has no repo")?;
            let value = api
                .get(&format!("/v1/orgs/{org}/repos/{repo}/vault/status"))
                .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }
    Ok(())
}
