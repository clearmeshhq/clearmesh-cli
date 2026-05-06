use crate::api::ApiClient;
use crate::config;
use crate::repo as local_repo;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::json;

#[derive(Args)]
pub struct RepoCommand {
    #[command(subcommand)]
    pub command: RepoSubcommand,
}

#[derive(Subcommand)]
pub enum RepoSubcommand {
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "private")]
        visibility: String,
        #[arg(long = "encryption", default_value = "required")]
        encryption_mode: String,
    },
    Update {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        visibility: Option<String>,
        #[arg(long = "encryption")]
        encryption_mode: Option<String>,
    },
    List,
    Use {
        repo: String,
    },
    Show,
    Archive,
    Unarchive,
    Delete {
        #[arg(long)]
        confirm: String,
    },
    Restore,
}

/// Resolve (org, repo) by checking local repo config first, then global config.
fn resolve_org_repo(cfg: &config::GlobalConfig) -> Result<(String, String)> {
    let (local_org, local_repo_slug) = match local_repo::try_discover() {
        Some(ref root) => {
            let local = local_repo::load_config(root)?;
            (local.org, local.repo)
        }
        None => (None, None),
    };
    let org = local_org
        .or_else(|| cfg.current_org.clone())
        .context("run clearmesh org use ORG first")?;
    let repo = local_repo_slug
        .or_else(|| cfg.current_repo.clone())
        .context("run clearmesh repo use REPO first")?;
    Ok((org, repo))
}

pub async fn run(command: RepoCommand) -> Result<()> {
    let mut cfg = config::load()?;
    let api = ApiClient::from_config(&cfg);
    match command.command {
        RepoSubcommand::Create {
            slug,
            name,
            description,
            visibility,
            encryption_mode,
        } => {
            let org = cfg
                .current_org
                .clone()
                .context("run clearmesh org use ORG first")?;

            let value = api
                .post(
                    &format!("/v1/orgs/{org}/repos"),
                    &json!({ "slug": slug, "name": name, "description": description, "visibility": visibility, "encryption_mode": encryption_mode }),
                )
                .await?;

            let created_slug = value["repo"]["slug"].as_str().unwrap_or(&slug).to_string();

            cfg.current_org = Some(org.clone());
            cfg.current_repo = Some(created_slug.clone());
            config::save(&cfg)?;

            if let Some(root) = local_repo::try_discover() {
                let mut local = local_repo::load_config(&root)?;
                local.org = Some(org.clone());
                local.repo = Some(created_slug.clone());
                local_repo::save_config_at(&root, &local)?;
            }

            println!("{}", serde_json::to_string_pretty(&value)?);
            println!("Using repo {org}/{created_slug}");
            println!("Next:");
            if local_repo::try_discover().is_none() {
                println!("  clearmesh init");
            }
            if encryption_mode == "required" {
                println!("  clearmesh commit --message \"initial commit\" --key <passphrase>");
            } else {
                println!("  clearmesh commit --message \"initial commit\"");
            }
            println!("  clearmesh push");
        }
        RepoSubcommand::Update {
            name,
            description,
            visibility,
            encryption_mode,
        } => {
            let (org, repo) = resolve_org_repo(&cfg)?;
            // Only send fields that were explicitly set
            let mut body = serde_json::Map::new();
            if let Some(ref v) = name {
                body.insert("name".into(), serde_json::Value::String(v.clone()));
            }
            if let Some(ref v) = description {
                body.insert("description".into(), serde_json::Value::String(v.clone()));
            }
            if let Some(ref v) = visibility {
                body.insert("visibility".into(), serde_json::Value::String(v.clone()));
            }
            if let Some(ref v) = encryption_mode {
                body.insert(
                    "encryption_mode".into(),
                    serde_json::Value::String(v.clone()),
                );
            }
            let value = api
                .patch(
                    &format!("/v1/orgs/{org}/repos/{repo}"),
                    &serde_json::Value::Object(body),
                )
                .await?;
            let r = &value["repo"];
            println!("Updated {org}/{repo}");
            println!("  Name:       {}", r["name"].as_str().unwrap_or("—"));
            println!("  Visibility: {}", r["visibility"].as_str().unwrap_or("—"));
            println!(
                "  Encryption: {}",
                r["encryption_mode"].as_str().unwrap_or("—")
            );
            if let Some(desc) = r["description"].as_str().filter(|s| !s.is_empty()) {
                println!("  Desc:       {desc}");
            }
        }
        RepoSubcommand::List => {
            let org = cfg
                .current_org
                .clone()
                .context("run clearmesh org use ORG first")?;
            let value = api.get(&format!("/v1/orgs/{org}/repos")).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        RepoSubcommand::Use { repo } => {
            let repo = repo.trim_matches('/').to_string();
            let (input_org, repo) = match repo.split_once('/') {
                Some((org, repo)) if !org.is_empty() && !repo.is_empty() => {
                    (Some(org.to_string()), repo.to_string())
                }
                _ => (None, repo),
            };
            if let Some(org) = input_org {
                cfg.current_org = Some(org);
            }
            cfg.current_repo = Some(repo.clone());
            config::save(&cfg)?;
            if let Some(root) = local_repo::try_discover() {
                let mut local = local_repo::load_config(&root)?;
                local.org = cfg.current_org.clone().or(local.org);
                local.repo = Some(repo.clone());
                local_repo::save_config_at(&root, &local)?;
            }
            let current_org = cfg.current_org.as_deref().unwrap_or("(no org)");
            println!("Using repo {current_org}/{repo}");
        }
        RepoSubcommand::Show => {
            let (org, repo) = resolve_org_repo(&cfg)?;
            let value = api.get(&format!("/v1/orgs/{org}/repos/{repo}")).await?;
            let repo_value = &value["repo"];
            let visibility = repo_value["visibility"].as_str().unwrap_or("private");
            let encryption = repo_value["encryption_mode"].as_str().unwrap_or("required");
            let default_branch = repo_value["default_branch"].as_str().unwrap_or("main");
            let commit_count = repo_value["commit_count"].as_i64().unwrap_or_default();
            println!("{org}/{repo}");
            println!("Visibility: {visibility}");
            println!("Encryption: {encryption}");
            println!("Default branch: {default_branch}");
            println!("Commits: {commit_count}");
            if encryption == "none" {
                println!("Clone: clearmesh clone {org}/{repo} ./{repo}");
            } else {
                println!("Clone: clearmesh clone {org}/{repo} ./{repo} --key \"$PASSPHRASE\"");
            }
        }
        RepoSubcommand::Archive => repo_action(&api, &cfg, "archive", true).await?,
        RepoSubcommand::Unarchive => repo_action(&api, &cfg, "unarchive", false).await?,
        RepoSubcommand::Delete { confirm } => {
            let (org, repo) = resolve_org_repo(&cfg)?;
            if confirm != repo {
                anyhow::bail!("--confirm value must exactly match the repo slug '{repo}'");
            }
            api.delete(&format!("/v1/orgs/{org}/repos/{repo}")).await?;
            println!(
                "Repository {repo} soft-deleted. Vault objects were retained. Local files were not deleted."
            );
        }
        RepoSubcommand::Restore => repo_action(&api, &cfg, "restore", false).await?,
    }
    Ok(())
}

async fn repo_action(
    api: &ApiClient,
    cfg: &config::GlobalConfig,
    action: &str,
    patch: bool,
) -> Result<()> {
    let (org, repo) = resolve_org_repo(cfg)?;
    let path = format!("/v1/orgs/{org}/repos/{repo}/{action}");
    let value = if patch {
        api.patch(&path, &json!({})).await?
    } else {
        api.post(&path, &json!({})).await?
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
