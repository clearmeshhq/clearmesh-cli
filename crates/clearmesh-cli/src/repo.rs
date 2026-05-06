use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

fn default_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalRepoConfig {
    pub org: Option<String>,
    pub repo: Option<String>,
    #[serde(default = "default_branch")]
    pub branch: String,
}

impl Default for LocalRepoConfig {
    fn default() -> Self {
        Self {
            org: None,
            repo: None,
            branch: default_branch(),
        }
    }
}

pub fn init_at(path: &Path) -> Result<()> {
    let root = path.join(".clearmesh");
    fs::create_dir_all(root.join("objects"))?;
    fs::create_dir_all(root.join("commits"))?;
    fs::create_dir_all(root.join("refs/heads"))?;
    fs::write(root.join("HEAD"), b"refs/heads/main\n")?;
    fs::write(root.join("refs/heads/main"), b"")?;
    save_config_at(path, &LocalRepoConfig::default())?;
    Ok(())
}

pub fn discover() -> Result<PathBuf> {
    try_discover().ok_or_else(|| anyhow!("not inside a ClearMesh repository"))
}

pub fn try_discover() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".clearmesh/HEAD").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub fn load_config(root: &Path) -> Result<LocalRepoConfig> {
    let path = root.join(".clearmesh/config.json");
    if !path.exists() {
        return Ok(LocalRepoConfig::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn save_config_at(root: &Path, config: &LocalRepoConfig) -> Result<()> {
    guard_local_root(root)?;
    let path = root.join(".clearmesh/config.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(config)?)?;
    Ok(())
}

fn guard_local_root(root: &Path) -> Result<()> {
    let home = dirs::home_dir().context("home directory not found")?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let home = home.canonicalize().unwrap_or(home);
    if root == home || root == home.join(".clearmesh") {
        anyhow::bail!("refusing to write local repo config to global ClearMesh config location");
    }
    Ok(())
}

pub fn object_path(root: &Path, hash: &str) -> PathBuf {
    let prefix = hash.get(..2).unwrap_or("xx");
    root.join(".clearmesh/objects").join(prefix).join(hash)
}

pub fn write_object(root: &Path, hash: &str, bytes: &[u8]) -> Result<()> {
    let path = object_path(root, hash);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

pub fn read_object(root: &Path, hash: &str) -> Result<Vec<u8>> {
    let path = object_path(root, hash);
    fs::read(&path).with_context(|| format!("read {}", path.display()))
}

pub fn write_commit(root: &Path, commit: &clearmesh_core::Commit) -> Result<()> {
    let path = root
        .join(".clearmesh/commits")
        .join(format!("{}.json", commit.id));
    fs::write(path, serde_json::to_vec_pretty(commit)?)?;
    Ok(())
}

pub fn read_commit(root: &Path, commit_id: &str) -> Result<clearmesh_core::Commit> {
    let path = root
        .join(".clearmesh/commits")
        .join(format!("{commit_id}.json"));
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn current_ref(root: &Path) -> Result<String> {
    let head = fs::read_to_string(root.join(".clearmesh/HEAD"))?;
    Ok(head.trim().to_string())
}

pub fn current_commit_id(root: &Path) -> Result<Option<String>> {
    let reference = current_ref(root)?;
    let path = root.join(".clearmesh").join(reference);
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)?.trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

pub fn update_head(root: &Path, commit_id: &str) -> Result<()> {
    let branch = current_branch(root)?;
    update_branch_head(root, &branch, commit_id)
}

pub fn update_branch_head(root: &Path, branch: &str, commit_id: &str) -> Result<()> {
    let reference = format!("refs/heads/{branch}");
    let path = root.join(".clearmesh").join(reference);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{commit_id}\n"))?;
    Ok(())
}

pub fn current_branch(root: &Path) -> Result<String> {
    let reference = current_ref(root)?;
    Ok(reference
        .strip_prefix("refs/heads/")
        .unwrap_or("main")
        .to_string())
}

pub fn set_current_branch(root: &Path, branch: &str) -> Result<()> {
    fs::write(
        root.join(".clearmesh/HEAD"),
        format!("refs/heads/{branch}\n").as_bytes(),
    )?;
    fs::create_dir_all(root.join(".clearmesh/refs/heads"))?;
    let branch_ref = root.join(".clearmesh/refs/heads").join(branch);
    if !branch_ref.exists() {
        fs::write(branch_ref, b"")?;
    }
    Ok(())
}

// File index caching support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCacheEntry {
    pub path: String,
    pub size: u64,
    pub mtime_ns: u64,
    pub file_hash: String,
    pub chunk_size: usize,
    pub chunk_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileIndex {
    pub entries: std::collections::HashMap<String, FileCacheEntry>,
}

pub fn file_index_path(root: &Path) -> PathBuf {
    root.join(".clearmesh/file-index.json")
}

pub fn load_file_index(root: &Path) -> Result<FileIndex> {
    let path = file_index_path(root);
    if !path.exists() {
        return Ok(FileIndex::default());
    }
    Ok(serde_json::from_slice(&fs::read(&path)?)?)
}

pub fn save_file_index(root: &Path, index: &FileIndex) -> Result<()> {
    let path = file_index_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(index)?)?;
    Ok(())
}

pub fn get_file_mtime_ns(path: &Path) -> Result<u64> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;
    let duration = modified.duration_since(SystemTime::UNIX_EPOCH)?;
    Ok(duration.as_nanos() as u64)
}
