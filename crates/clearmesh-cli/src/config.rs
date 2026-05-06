use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn default_api() -> String {
    "https://api.clearmesh.net".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    #[serde(default = "default_api")]
    pub api: String,
    pub token: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub current_org: Option<String>,
    pub current_repo: Option<String>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            api: default_api(),
            token: None,
            email: None,
            name: None,
            current_org: None,
            current_repo: None,
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("home directory not found")?
        .join(".clearmesh"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load() -> Result<GlobalConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)?;
    if looks_like_local_repo_config(&value) {
        let backup = config_dir()?.join("config.local-misplaced.bak.json");
        fs::write(&backup, &bytes).with_context(|| format!("write {}", backup.display()))?;
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        eprintln!(
            "Detected local repo config in global config path; backed it up and reset global config."
        );
        return Ok(GlobalConfig::default());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save(config: &GlobalConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(config)?)?;
    Ok(())
}

fn looks_like_local_repo_config(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let has_local = object.contains_key("org")
        || object.contains_key("repo")
        || (object.contains_key("branch")
            && !object.contains_key("api")
            && !object.contains_key("token")
            && !object.contains_key("email")
            && !object.contains_key("current_org"));
    let has_global = object.contains_key("api")
        || object.contains_key("token")
        || object.contains_key("email")
        || object.contains_key("name")
        || object.contains_key("current_org")
        || object.contains_key("current_repo");
    has_local && !has_global
}
