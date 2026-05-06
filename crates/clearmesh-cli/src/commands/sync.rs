use crate::api::ApiClient;
use crate::commands::commit::require_local_repo;
use crate::config;
use crate::repo as local_repo;
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use clearmesh_core::{
    decrypt_chunk, demo_key_from_passphrase, hash_bytes, ChunkRef, Commit, FileEntry,
};
use serde_json::json;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args, Clone)]
pub struct KeyArgs {
    #[arg(long)]
    pub key: Option<String>,
}

#[derive(Args)]
pub struct CloneArgs {
    pub target: String,
    pub path: PathBuf,
    #[arg(long)]
    pub key: Option<String>,
}

#[derive(Args)]
pub struct BranchCommand {
    #[command(subcommand)]
    pub command: BranchSubcommand,
}

#[derive(Subcommand)]
pub enum BranchSubcommand {
    List,
    Create { name: String },
    Switch { name: String },
    Delete { name: String },
}

const UPLOAD_CONCURRENCY: usize = 8;

pub async fn push(_args: KeyArgs) -> Result<()> {
    let push_start = std::time::Instant::now();
    let mut timings = std::collections::HashMap::new();

    let load_start = std::time::Instant::now();
    let (root, mut local) = require_local_repo()?;
    let cfg = config::load()?;
    timings.insert("load_config", load_start.elapsed());

    let logged_in_email = cfg
        .email
        .as_deref()
        .filter(|email| !email.trim().is_empty())
        .context("login required before pushing; run clearmesh login")?;
    cfg.token
        .as_deref()
        .filter(|token| !token.trim().is_empty())
        .context("login required before pushing; run clearmesh login")?;
    let api = ApiClient::from_config(&cfg);
    let branch = local_repo::current_branch(&root)?;
    if local.branch != branch {
        local.branch = branch.clone();
        local_repo::save_config_at(&root, &local)?;
    }
    let org = local
        .org
        .context("local repo has no org; run clearmesh org use ORG before clearmesh init, or run clearmesh repo use ORG/REPO")?;
    let repo = local
        .repo
        .context("local repo has no repo; run clearmesh repo use REPO")?;
    let commit_id = local_repo::current_commit_id(&root)?.context("nothing to push")?;

    let manifest_start = std::time::Instant::now();
    let commit = local_repo::read_commit(&root, &commit_id)?;
    let repo_value = api.get(&format!("/v1/orgs/{org}/repos/{repo}")).await?;
    timings.insert("manifest", manifest_start.elapsed());

    let encryption_mode = repo_value["repo"]["encryption_mode"]
        .as_str()
        .unwrap_or("required")
        .to_string();
    if commit.author_email.trim() != logged_in_email.trim() {
        bail!(
            "commit author {} does not match logged-in email {}; create a new commit after login",
            commit.author_email,
            logged_in_email
        );
    }

    let chunk_refs: Vec<&ChunkRef> = commit
        .files
        .iter()
        .flat_map(|file| file.chunks.iter())
        .collect();
    let total_chunks = chunk_refs.len();
    let total_bytes: u64 = chunk_refs.iter().map(|chunk| chunk.size).sum();
    let unique_hashes: Vec<String> = chunk_refs
        .iter()
        .map(|chunk| chunk.hash.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    println!(
        "Checking {} unique chunk{} ({})…",
        unique_hashes.len(),
        if unique_hashes.len() == 1 { "" } else { "s" },
        format_bytes(total_bytes)
    );

    let check_start = std::time::Instant::now();
    let check = api
        .post(
            &format!("/v1/orgs/{org}/repos/{repo}/vault/chunks/check"),
            &json!({ "chunks": unique_hashes }),
        )
        .await
        .context("chunk dedupe check failed")?;
    timings.insert("check_existing", check_start.elapsed());

    let existing: HashSet<String> = check["existing"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|hash| hash.as_str().map(ToOwned::to_owned))
        .collect();

    // Collect unique chunks that need uploading
    let mut to_upload: Vec<(String, u64, Vec<u8>)> = Vec::new();
    let mut seen_upload: HashSet<String> = HashSet::new();
    for file in &commit.files {
        for chunk in &file.chunks {
            if existing.contains(&chunk.hash) || !seen_upload.insert(chunk.hash.clone()) {
                continue;
            }
            let bytes = local_repo::read_object(&root, &chunk.hash)?;
            to_upload.push((chunk.hash.clone(), chunk.size, bytes));
        }
    }

    let needs_upload = to_upload.len();
    if needs_upload > 0 {
        println!(
            "Uploading {} chunk{} ({})…",
            needs_upload,
            if needs_upload == 1 { "" } else { "s" },
            format_bytes(to_upload.iter().map(|(_, s, _)| s).sum::<u64>())
        );
    }

    let upload_start = std::time::Instant::now();
    let mut active: tokio::task::JoinSet<Result<u64>> = tokio::task::JoinSet::new();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(UPLOAD_CONCURRENCY));
    let mut uploaded = 0usize;
    let mut uploaded_bytes = 0u64;

    for (hash, size, bytes) in to_upload {
        // Wait if we are at max concurrency
        while active.len() >= UPLOAD_CONCURRENCY {
            if let Some(res) = active.join_next().await {
                uploaded_bytes += res.context("upload task panicked")??;
                uploaded += 1;
            }
        }
        let api2 = api.clone();
        let org2 = org.clone();
        let repo2 = repo.clone();
        let sem2 = sem.clone();
        active.spawn(async move {
            let _permit = sem2.acquire_owned().await.unwrap();
            upload_encrypted_chunk(&api2, &org2, &repo2, &hash, bytes).await?;
            Ok(size)
        });
    }
    while let Some(res) = active.join_next().await {
        uploaded_bytes += res.context("upload task panicked")??;
        uploaded += 1;
    }
    timings.insert("upload", upload_start.elapsed());

    let reused = total_chunks.saturating_sub(uploaded);
    let dedup_saved_bytes = total_bytes.saturating_sub(uploaded_bytes);

    let commit_start = std::time::Instant::now();
    api.post(
        &format!("/v1/orgs/{org}/repos/{repo}/commits"),
        &json!({
            "id": commit.id,
            "message": commit.message,
            "author_email": commit.author_email,
            "parent_ids": commit.parent_ids,
            "files": commit.files,
            "branch": branch,
        }),
    )
    .await?;
    timings.insert("create_commit", commit_start.elapsed());

    let total_elapsed = push_start.elapsed().as_secs_f64();
    let throughput = if uploaded_bytes > 0 {
        (uploaded_bytes as f64 / 1024.0 / 1024.0) / total_elapsed
    } else {
        0.0
    };

    println!("Pushed commit {}", &commit.id[..16]);
    println!("  Branch:   {branch}");
    println!("  Files:    {}", commit.files.len());
    println!("  Chunks:   {total_chunks} total, {uploaded} uploaded, {reused} reused");
    println!("  Size:     {}", format_bytes(total_bytes));
    if dedup_saved_bytes > 0 {
        println!("  Dedup:    {} saved", format_bytes(dedup_saved_bytes));
    }
    if encryption_mode != "none" {
        println!("  Encrypt:  required (ChaCha20-Poly1305)");
    } else {
        println!("  Encrypt:  none");
    }
    println!("  Concurrency: {UPLOAD_CONCURRENCY}");
    println!("  Throughput: {throughput:.2} MiB/s");
    println!("  === Timing Breakdown ===");
    println!(
        "  Load config:     {:.3}s",
        timings
            .get("load_config")
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    );
    println!(
        "  Manifest fetch:  {:.3}s",
        timings
            .get("manifest")
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    );
    println!(
        "  Check existing:  {:.3}s",
        timings
            .get("check_existing")
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    );
    println!(
        "  Upload chunks:   {:.3}s",
        timings
            .get("upload")
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    );
    println!(
        "  Create commit:   {:.3}s",
        timings
            .get("create_commit")
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    );
    println!("  Total:           {total_elapsed:.3}s");
    Ok(())
}

pub async fn clone_repo(args: CloneArgs) -> Result<()> {
    let clone_start = std::time::Instant::now();
    let (org, repo) = args
        .target
        .split_once('/')
        .context("clone target must be ORG/REPO")?;
    fs::create_dir_all(&args.path)?;
    local_repo::init_at(&args.path)?;
    let mut local = local_repo::load_config(&args.path)?;
    local.org = Some(org.to_string());
    local.repo = Some(repo.to_string());
    local.branch = "main".to_string();
    local_repo::save_config_at(&args.path, &local)?;
    local_repo::set_current_branch(&args.path, "main")?;
    std::env::set_current_dir(&args.path)?;
    sync(KeyArgs { key: args.key }).await?;
    let elapsed = clone_start.elapsed().as_secs_f64();
    println!(
        "Cloned {org}/{repo} into {} ({elapsed:.1}s)",
        args.path.display()
    );
    Ok(())
}

pub async fn sync(args: KeyArgs) -> Result<()> {
    let sync_start = std::time::Instant::now();
    let (root, mut local) = require_local_repo()?;
    let cfg = config::load()?;
    let api = ApiClient::from_config(&cfg);
    let org = local
        .org
        .clone()
        .context("local repo has no org; run clearmesh org use ORG and clearmesh repo use REPO")?;
    let repo = local
        .repo
        .clone()
        .context("local repo has no repo; run clearmesh repo use REPO")?;
    let branch = local_repo::current_branch(&root)?;
    let repo_value = api.get(&format!("/v1/orgs/{org}/repos/{repo}")).await?;
    let encryption_mode = repo_value["repo"]["encryption_mode"]
        .as_str()
        .unwrap_or("required")
        .to_string();
    let key = match encryption_mode.as_str() {
        "required" => Some(demo_key_from_passphrase(
            args.key
                .as_deref()
                .context("--key is required for encrypted repositories")?,
        )),
        "none" => None,
        _ => bail!("unsupported repo encryption mode {encryption_mode}"),
    };
    if local.branch != branch {
        local.branch = branch.clone();
        local_repo::save_config_at(&root, &local)?;
    }
    let branch_value = api
        .get(&format!("/v1/orgs/{org}/repos/{repo}/branches/{branch}"))
        .await?;
    let Some(commit_id) = branch_value["branch"]["head_commit_id"].as_str() else {
        println!("Branch {branch} has no commits");
        return Ok(());
    };
    let local_head = local_repo::current_commit_id(&root)?;
    if local_head.as_deref() == Some(commit_id) {
        println!("Already up to date");
        return Ok(());
    }
    if let Some(local_head) = local_head.as_deref() {
        if is_ancestor(&api, &root, &org, &repo, commit_id, local_head).await? {
            println!("Local branch ahead of remote");
            return Ok(());
        }
        if !is_ancestor(&api, &root, &org, &repo, local_head, commit_id).await? {
            println!("branch has diverged; merge support coming next");
            return Ok(());
        }
        ensure_clean_worktree(&root, local_head, key.as_ref())?;
    }
    let tree = api
        .get(&format!(
            "/v1/orgs/{org}/repos/{repo}/commits/{commit_id}/tree"
        ))
        .await?;
    let files = files_from_tree(&tree)?;
    remove_deleted_files(
        &root,
        local_repo::current_commit_id(&root)?.as_deref(),
        &files,
    )?;

    // Collect all chunks that need downloading
    let mut chunks_to_download = Vec::new();
    for file in &files {
        for (chunk_idx, chunk) in file.chunks.iter().enumerate() {
            chunks_to_download.push((file.path.clone(), chunk_idx, chunk.clone()));
        }
    }

    let download_start = std::time::Instant::now();
    let total_chunks = chunks_to_download.len();
    if total_chunks > 0 {
        println!("Downloading {} chunks…", total_chunks);
    }

    // Download chunks in parallel with bounded concurrency
    const DOWNLOAD_CONCURRENCY: usize = 8;
    let mut active: tokio::task::JoinSet<Result<(String, usize, Vec<u8>)>> =
        tokio::task::JoinSet::new();
    let mut chunk_results = std::collections::HashMap::new();

    for (file_path, chunk_idx, chunk) in chunks_to_download {
        while active.len() >= DOWNLOAD_CONCURRENCY {
            if let Some(res) = active.join_next().await {
                let (path, idx, bytes) = res.context("download task panicked")??;
                chunk_results
                    .entry(path)
                    .or_insert_with(Vec::new)
                    .push((idx, bytes));
            }
        }

        let api2 = api.clone();
        let org2 = org.clone();
        let repo2 = repo.clone();
        let root2 = root.clone();

        active.spawn(async move {
            let stored = download_encrypted_chunk(&api2, &org2, &repo2, &chunk.hash).await?;
            local_repo::write_object(&root2, &chunk.hash, &stored)?;
            Ok((file_path, chunk_idx, stored))
        });
    }

    // Collect remaining downloads
    while let Some(res) = active.join_next().await {
        let (path, idx, bytes) = res.context("download task panicked")??;
        chunk_results
            .entry(path)
            .or_insert_with(Vec::new)
            .push((idx, bytes));
    }
    let download_elapsed = download_start.elapsed();

    // Decrypt and write files atomically (temp file + rename)
    let write_start = std::time::Instant::now();
    for file in &files {
        if file.size > 0 && file.chunks.is_empty() {
            eprintln!(
                "warning: remote file '{}' has size {} but no chunks; skipping",
                file.path, file.size
            );
            continue;
        }

        let mut plaintext = Vec::new();

        if let Some(chunks) = chunk_results.get(&file.path) {
            let mut sorted_chunks = chunks.clone();
            sorted_chunks.sort_by_key(|c| c.0);

            for (_idx, stored) in sorted_chunks {
                match key.as_ref() {
                    Some(key) => plaintext.extend(decrypt_chunk(key, &stored)?),
                    None => plaintext.extend(stored),
                }
            }
        }

        let out = root.join(&file.path);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        // Write to a temp file then atomically rename to avoid partial writes on interruption.
        let tmp = out.with_extension("clearmesh-tmp");
        fs::write(&tmp, &plaintext)?;
        fs::rename(&tmp, &out)?;
    }
    let write_elapsed = write_start.elapsed();

    // Calculate metrics before moving files
    let total_bytes: u64 = files.iter().flat_map(|f| &f.chunks).map(|c| c.size).sum();

    let commit_value = api
        .get(&format!("/v1/orgs/{org}/repos/{repo}/commits/{commit_id}"))
        .await?;
    let commit = Commit {
        id: commit_id.to_string(),
        message: commit_value["commit"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        author_email: commit_value["commit"]["author_email"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        created_at: commit_value["commit"]["created_at"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(chrono::Utc::now),
        parent_ids: commit_value["commit"]["parent_ids"]
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|p| p.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        files,
    };
    local_repo::write_commit(&root, &commit)?;
    local_repo::update_branch_head(&root, &branch, commit_id)?;

    let total_elapsed = sync_start.elapsed().as_secs_f64();
    let download_throughput = if download_elapsed.as_secs_f64() > 0.0 {
        (total_bytes as f64 / 1024.0 / 1024.0) / download_elapsed.as_secs_f64()
    } else {
        0.0
    };

    println!(
        "Fast-forwarded {branch} to {} ({:.1}s)",
        &commit_id[..16],
        total_elapsed
    );
    println!("  Chunks:      {total_chunks} downloaded");
    println!("  Concurrency: {DOWNLOAD_CONCURRENCY}");
    println!("  Throughput:  {download_throughput:.2} MiB/s");
    println!("  === Timing Breakdown ===");
    println!("  Download:    {:.3}s", download_elapsed.as_secs_f64());
    println!("  Decrypt/write: {:.3}s", write_elapsed.as_secs_f64());
    println!("  Total:       {total_elapsed:.3}s");
    Ok(())
}

async fn upload_encrypted_chunk(
    api: &ApiClient,
    org: &str,
    repo: &str,
    hash: &str,
    bytes: Vec<u8>,
) -> Result<()> {
    let signed = api
        .post(
            &format!("/v1/orgs/{org}/repos/{repo}/vault/chunks/{hash}/presign-put"),
            &json!({}),
        )
        .await
        .context("direct Vault upload failed: presign request failed")?;
    let signed = serde_json::from_value(signed)
        .context("direct Vault upload failed: invalid presign response")?;
    let size = bytes.len() as i64;
    api.put_presigned(&signed, bytes)
        .await
        .context("direct Vault upload failed")?;
    api.post(
        &format!("/v1/orgs/{org}/repos/{repo}/vault/chunks/{hash}/confirm"),
        &json!({ "size": size }),
    )
    .await
    .context("direct Vault upload failed: confirm request failed")?;
    Ok(())
}

async fn download_encrypted_chunk(
    api: &ApiClient,
    org: &str,
    repo: &str,
    hash: &str,
) -> Result<Vec<u8>> {
    let signed = api
        .post(
            &format!("/v1/orgs/{org}/repos/{repo}/vault/chunks/{hash}/presign-get"),
            &json!({}),
        )
        .await
        .context("direct Vault download failed: presign request failed")?;
    let signed = serde_json::from_value(signed)
        .context("direct Vault download failed: invalid presign response")?;
    let encrypted = api
        .get_presigned(&signed)
        .await
        .context("direct Vault download failed")?;
    if hash_bytes(&encrypted) != hash {
        bail!("direct Vault download failed: chunk hash mismatch");
    }
    let _ = api
        .post(
            &format!("/v1/orgs/{org}/repos/{repo}/usage/chunk-download"),
            &json!({ "hash": hash, "bytes": encrypted.len(), "source": "direct" }),
        )
        .await;
    Ok(encrypted)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn files_from_tree(tree: &serde_json::Value) -> Result<Vec<FileEntry>> {
    tree["files"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|file| {
            let path = file["path"].as_str().context("file path missing")?;
            let chunks = file["chunks"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|chunk| {
                    Ok(ChunkRef {
                        hash: chunk["hash"]
                            .as_str()
                            .context("chunk hash missing")?
                            .to_string(),
                        size: chunk["size"].as_i64().unwrap_or_default() as u64,
                        index: chunk["index"].as_i64().unwrap_or_default() as u32,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(FileEntry {
                path: path.to_string(),
                size: file["size"].as_i64().unwrap_or_default() as u64,
                mode: file["mode"].as_i64().unwrap_or(0o100644) as u32,
                chunks,
                chunk_size: 1024 * 1024,
            })
        })
        .collect()
}

fn ensure_clean_worktree(root: &Path, head: &str, key: Option<&[u8; 32]>) -> Result<()> {
    let commit = local_repo::read_commit(root, head)?;
    let expected_paths: HashSet<&str> = commit.files.iter().map(|f| f.path.as_str()).collect();
    let actual_files = crate::commands::commit::working_files(root)?;
    let actual_paths: HashSet<String> = actual_files
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    if expected_paths.len() != actual_paths.len()
        || expected_paths.iter().any(|p| !actual_paths.contains(*p))
    {
        bail!("working tree has uncommitted changes; commit or remove them before sync");
    }

    for file in &commit.files {
        let mut expected = Vec::new();
        for chunk in &file.chunks {
            let stored = local_repo::read_object(root, &chunk.hash)?;
            match key {
                Some(key) => expected.extend(decrypt_chunk(key, &stored)?),
                None => expected.extend(stored),
            }
        }
        let actual = fs::read(root.join(&file.path))?;
        if expected != actual {
            bail!("working tree has uncommitted changes; commit or remove them before sync");
        }
    }
    Ok(())
}

fn remove_deleted_files(
    root: &Path,
    old_head: Option<&str>,
    new_files: &[FileEntry],
) -> Result<()> {
    let Some(old_head) = old_head else {
        return Ok(());
    };
    let old_commit = local_repo::read_commit(root, old_head)?;
    let new_paths: HashSet<&str> = new_files.iter().map(|f| f.path.as_str()).collect();
    for file in old_commit.files {
        if !new_paths.contains(file.path.as_str()) {
            let path = root.join(file.path);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

async fn is_ancestor(
    api: &ApiClient,
    root: &Path,
    org: &str,
    repo: &str,
    ancestor: &str,
    descendant: &str,
) -> Result<bool> {
    if ancestor == descendant {
        return Ok(true);
    }
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([descendant.to_string()]);
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for parent in commit_parents(api, root, org, repo, &id).await? {
            if parent == ancestor {
                return Ok(true);
            }
            queue.push_back(parent);
        }
    }
    Ok(false)
}

async fn commit_parents(
    api: &ApiClient,
    root: &Path,
    org: &str,
    repo: &str,
    commit_id: &str,
) -> Result<Vec<String>> {
    if let Ok(commit) = local_repo::read_commit(root, commit_id) {
        return Ok(commit.parent_ids);
    }
    let value = api
        .get(&format!("/v1/orgs/{org}/repos/{repo}/commits/{commit_id}"))
        .await?;
    Ok(value["commit"]["parent_ids"]
        .as_array()
        .map(|parents| {
            parents
                .iter()
                .filter_map(|p| p.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default())
}

pub fn log() -> Result<()> {
    let (root, _) = require_local_repo()?;
    for entry in fs::read_dir(root.join(".clearmesh/commits"))? {
        let entry = entry?;
        if entry.path().extension().and_then(|v| v.to_str()) == Some("json") {
            let commit: Commit = serde_json::from_slice(&fs::read(entry.path())?)?;
            println!("{} {}", commit.id, commit.message);
        }
    }
    Ok(())
}

pub async fn branch(command: BranchCommand) -> Result<()> {
    let (root, mut local) = require_local_repo()?;
    let cfg = config::load()?;
    let api = ApiClient::from_config(&cfg);
    let org = local.org.clone().context("local repo has no org")?;
    let repo = local.repo.clone().context("local repo has no repo")?;
    match command.command {
        BranchSubcommand::List => {
            let value = api
                .get(&format!("/v1/orgs/{org}/repos/{repo}/branches"))
                .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        BranchSubcommand::Create { name } => {
            let head = local_repo::current_commit_id(&root)?;
            let value = api
                .post(
                    &format!("/v1/orgs/{org}/repos/{repo}/branches"),
                    &json!({ "name": name, "head_commit_id": head }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        BranchSubcommand::Switch { name } => {
            let value = api
                .get(&format!("/v1/orgs/{org}/repos/{repo}/branches/{name}"))
                .await?;
            local.branch = name.clone();
            local_repo::save_config_at(&root, &local)?;
            local_repo::set_current_branch(&root, &name)?;
            if let Some(head) = value["branch"]["head_commit_id"].as_str() {
                local_repo::update_branch_head(&root, &name, head)?;
            }
            println!("Switched to {name}");
        }
        BranchSubcommand::Delete { name } => {
            let value = api
                .delete(&format!("/v1/orgs/{org}/repos/{repo}/branches/{name}"))
                .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }
    Ok(())
}
