use crate::api::ApiClient;
use crate::config;
use crate::repo as local_repo;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;
use clearmesh_core::{
    adaptive_chunk_size, demo_key_from_passphrase, encrypt_chunk, finalize_commit, hash_bytes,
    process_chunks_streaming, ChunkRef, Commit, FileEntry,
};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::time::Instant;
use walkdir::WalkDir;

#[derive(Args)]
pub struct CommitArgs {
    #[arg(short = 'm', long)]
    pub message: String,
    #[arg(long)]
    pub key: Option<String>,
}

pub fn init() -> Result<()> {
    let cwd = std::env::current_dir()?;
    local_repo::init_at(&cwd)?;
    let gcfg = config::load()?;
    if gcfg.current_org.is_some() || gcfg.current_repo.is_some() {
        let mut local = local_repo::load_config(&cwd)?;
        if local.org.is_none() {
            local.org = gcfg.current_org.clone();
        }
        if local.repo.is_none() {
            local.repo = gcfg.current_repo.clone();
        }
        local_repo::save_config_at(&cwd, &local)?;
    }
    let final_local = local_repo::load_config(&cwd)?;
    println!("Initialized ClearMesh repository in {}", cwd.display());
    if let (Some(org), Some(repo)) = (final_local.org.as_deref(), final_local.repo.as_deref()) {
        println!("Linked local repo to {org}/{repo}");
    } else {
        println!("No org/repo selected yet. Run: clearmesh repo use ORG/REPO");
    }
    Ok(())
}

pub fn status() -> Result<()> {
    let root = local_repo::discover()?;
    let branch = local_repo::current_branch(&root)?;
    let head = local_repo::current_commit_id(&root)?;
    let tracked_files = working_files(&root)?.len();
    println!("Repository: {}", root.display());
    println!("Branch: {branch}");
    println!("HEAD: {}", head.as_deref().unwrap_or("(none)"));
    println!("Working files: {tracked_files}");
    Ok(())
}

#[derive(Debug, Clone)]
struct ProcessedChunk {
    index: usize,
    hash: String,
    size: u64,
}

pub async fn commit(args: CommitArgs) -> Result<()> {
    let start_total = Instant::now();

    let root = local_repo::discover()?;
    let cfg = config::load()?;
    let author_email = cfg
        .email
        .clone()
        .filter(|email| !email.trim().is_empty())
        .context("login required before committing; run clearmesh login")?;
    cfg.token
        .as_ref()
        .filter(|token| !token.trim().is_empty())
        .context("login required before committing; run clearmesh login")?;
    let local = local_repo::load_config(&root)?;
    let org = local
        .org
        .as_deref()
        .context("local repo has no org; run clearmesh repo use ORG/REPO")?;
    let repo = local
        .repo
        .as_deref()
        .context("local repo has no repo; run clearmesh repo use REPO")?;
    let api = ApiClient::from_config(&cfg);
    let repo_value = api.get(&format!("/v1/orgs/{org}/repos/{repo}")).await?;
    let encryption_mode = repo_value["repo"]["encryption_mode"]
        .as_str()
        .unwrap_or("required");
    let key = match encryption_mode {
        "required" => Some(demo_key_from_passphrase(
            args.key
                .as_deref()
                .context("--key is required for encrypted repositories")?,
        )),
        "none" => None,
        _ => anyhow::bail!("unsupported repo encryption mode {encryption_mode}"),
    };
    let merge_state = crate::commands::merge::pending_merge(&root)?;
    if let Some(state) = merge_state.as_ref() {
        if state.conflicts.iter().any(|c| !c.resolved) {
            anyhow::bail!("merge has unresolved conflicts; run clearmesh conflicts");
        }
    }
    let parent_ids = if let Some(state) = merge_state.as_ref() {
        vec![state.ours.clone(), state.theirs.clone()]
    } else {
        local_repo::current_commit_id(&root)?.into_iter().collect()
    };

    let previous_files: HashMap<String, FileEntry> = parent_ids
        .first()
        .and_then(|id| local_repo::read_commit(&root, id).ok())
        .map(|commit| {
            commit
                .files
                .into_iter()
                .map(|file| (file.path.clone(), file))
                .collect()
        })
        .unwrap_or_default();

    let start_scan = Instant::now();
    let file_paths = working_files(&root)?;
    let scan_time = start_scan.elapsed();

    let mut files = Vec::new();
    let mut total_bytes_processed = 0u64;
    let mut total_chunks = 0u32;
    let mut files_unchanged = 0usize;

    let file_index = local_repo::load_file_index(&root).unwrap_or_default();
    let chunk_concurrency = std::env::var("CLEARMESH_CHUNK_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or_else(|| {
            let cpus = num_cpus::get();
            std::cmp::min(cpus, 8)
        });

    for file_path in &file_paths {
        let rel_path = file_path
            .strip_prefix(&root)?
            .to_string_lossy()
            .replace('\\', "/");

        let bytes = fs::read(file_path)?;
        let file_size = bytes.len() as u64;

        // Check if file is unchanged
        let file_hash = hash_bytes(&bytes);
        let mtime_ns = local_repo::get_file_mtime_ns(file_path).unwrap_or(0);
        let cache_entry = file_index.entries.get(&rel_path);

        let use_cache = cache_entry
            .map(|e| e.size == file_size && e.mtime_ns == mtime_ns && e.file_hash == file_hash)
            .unwrap_or(false);

        let chunk_size = adaptive_chunk_size(file_size);
        let mut chunks = Vec::new();

        if use_cache {
            if let Some(previous) = previous_files
                .get(&rel_path)
                .filter(|file| file.size == file_size && !file.chunks.is_empty())
            {
                chunks = previous.chunks.clone();
                files_unchanged += 1;
            } else {
                eprintln!(
                    "cache hit for {rel_path}, but no reusable chunks found in parent commit; rechunking"
                );
            }
        }

        if chunks.is_empty() {
            // Process file: read, chunk, hash, encrypt, write
            let raw_chunks: Result<Vec<_>> = {
                let mut result_chunks = Vec::new();
                process_chunks_streaming(&bytes, chunk_size, |idx, chunk| {
                    result_chunks.push((idx, chunk.to_vec()));
                    Ok(())
                })?;
                Ok(result_chunks)
            };

            let raw_chunks = raw_chunks?;
            let chunk_count = raw_chunks.len();

            // Parallel processing of chunks: hash and encrypt
            let processed: Result<Vec<_>> = raw_chunks
                .iter()
                .enumerate()
                .par_bridge()
                .map(|(idx, (_, chunk))| {
                    let stored = match key.as_ref() {
                        Some(key) => encrypt_chunk(key, chunk)?,
                        None => chunk.clone(),
                    };
                    let hash = hash_bytes(&stored);
                    Ok(ProcessedChunk {
                        index: idx,
                        hash: hash.clone(),
                        size: stored.len() as u64,
                    })
                })
                .collect();

            let processed_chunks = processed?;

            // Write chunks sequentially to preserve order and I/O efficiency
            for pchunk in &processed_chunks {
                // Re-encrypt for writing (must preserve deterministic encryption)
                let raw_chunk_data = &raw_chunks[pchunk.index].1;
                let stored = match key.as_ref() {
                    Some(key) => encrypt_chunk(key, raw_chunk_data)?,
                    None => raw_chunk_data.clone(),
                };
                local_repo::write_object(&root, &pchunk.hash, &stored)?;
            }

            // Sort by index and collect ChunkRefs
            let mut sorted_chunks: Vec<_> = processed_chunks.into_iter().collect();
            sorted_chunks.sort_by_key(|c| c.index);
            chunks = sorted_chunks
                .into_iter()
                .enumerate()
                .map(|(idx, pchunk)| ChunkRef {
                    hash: pchunk.hash,
                    size: pchunk.size,
                    index: idx as u32,
                })
                .collect();

            total_bytes_processed += file_size;
            total_chunks += chunk_count as u32;
        }

        files.push(FileEntry {
            path: rel_path,
            size: file_size,
            mode: 0o100644,
            chunks,
            chunk_size,
        });
    }

    let start_finalize = Instant::now();
    let commit = finalize_commit(Commit::new(
        args.message,
        author_email,
        Utc::now(),
        parent_ids,
        files.clone(),
    ));
    let finalize_time = start_finalize.elapsed();

    local_repo::write_commit(&root, &commit)?;
    local_repo::update_head(&root, &commit.id)?;

    // Update file index cache
    let mut new_index = local_repo::FileIndex::default();
    for file in &files {
        if let Ok(abs_path) = std::fs::canonicalize(root.join(&file.path)) {
            if let Ok(bytes) = fs::read(&abs_path) {
                if let Ok(mtime_ns) = local_repo::get_file_mtime_ns(&abs_path) {
                    new_index.entries.insert(
                        file.path.clone(),
                        local_repo::FileCacheEntry {
                            path: file.path.clone(),
                            size: file.size,
                            mtime_ns,
                            file_hash: hash_bytes(&bytes),
                            chunk_size: file.chunk_size,
                            chunk_count: file.chunks.len() as u32,
                        },
                    );
                }
            }
        }
    }
    let _ = local_repo::save_file_index(&root, &new_index);

    if merge_state.is_some() {
        crate::commands::merge::finish_merge(&root)?;
    }

    let total_time = start_total.elapsed();
    let throughput_mbs = if total_bytes_processed > 0 {
        (total_bytes_processed as f64 / 1024.0 / 1024.0) / total_time.as_secs_f64()
    } else {
        0.0
    };

    println!("Created commit {}", commit.id);
    println!("\n=== Timing Breakdown ===");
    println!("Files scanned: {}", file_paths.len());
    println!("Files unchanged/skipped: {}", files_unchanged);
    println!("Files processed: {}", file_paths.len() - files_unchanged);
    println!(
        "Bytes processed: {} MiB",
        total_bytes_processed / 1024 / 1024
    );
    println!("Chunks produced: {}", total_chunks);
    println!("Chunk concurrency: {}", chunk_concurrency);
    println!("Scan time: {:.3}s", scan_time.as_secs_f64());
    println!("Finalize time: {:.3}s", finalize_time.as_secs_f64());
    println!("Total time: {:.3}s", total_time.as_secs_f64());
    println!("Throughput: {:.2} MiB/s", throughput_mbs);

    Ok(())
}

pub fn working_files(root: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = entry?;
        let path = entry.path();
        if path.components().any(|c| c.as_os_str() == ".clearmesh") {
            continue;
        }
        if entry.file_type().is_file() {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    if files.is_empty() {
        println!("No working files found");
    }
    Ok(files)
}

pub fn require_local_repo() -> Result<(std::path::PathBuf, crate::repo::LocalRepoConfig)> {
    let root = local_repo::discover()?;
    let config = local_repo::load_config(&root).context("load local repo config")?;
    Ok((root, config))
}
