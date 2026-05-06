use crate::api::ApiClient;
use crate::config;
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::json;

#[derive(Args)]
pub struct OrgCommand {
    #[command(subcommand)]
    pub command: OrgSubcommand,
}

#[derive(Subcommand)]
pub enum OrgSubcommand {
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
    },
    Use {
        org: String,
    },
    List,
}

pub async fn run(command: OrgCommand) -> Result<()> {
    let mut cfg = config::load()?;
    let api = ApiClient::from_config(&cfg);
    match command.command {
        OrgSubcommand::Create { slug, name } => {
            let value = api
                .post("/v1/orgs", &json!({ "slug": slug, "name": name }))
                .await?;

            let created_slug = value["org"]["slug"].as_str().unwrap_or(&slug).to_string();

            cfg.current_org = Some(created_slug.clone());
            config::save(&cfg)?;

            println!("{}", serde_json::to_string_pretty(&value)?);
            println!("Using org {created_slug}");
            println!("Next: clearmesh repo create --slug dataset --name \"Dataset\"");
        }
        OrgSubcommand::Use { org } => {
            cfg.current_org = Some(org.clone());
            config::save(&cfg)?;
            println!("Using org {org}");
        }
        OrgSubcommand::List => {
            let value = api.get("/v1/orgs").await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }
    Ok(())
}
