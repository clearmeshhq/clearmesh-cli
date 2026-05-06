#[cfg(unix)]
use crate::api::ApiClient;
#[cfg(unix)]
use crate::config;
#[cfg(unix)]
use anyhow::Context;
use anyhow::{bail, Result};
use clap::Args;
#[cfg(unix)]
use clearmesh_core::{decrypt_chunk, demo_key_from_passphrase, hash_bytes, ChunkRef, FileEntry};
#[cfg(unix)]
use fuser::{
    BackgroundSession, FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData,
    ReplyDirectory, ReplyEntry, ReplyOpen, Request,
};
#[cfg(unix)]
use libc::{EACCES, EIO, ENOENT, EROFS, O_ACCMODE, O_RDONLY};
#[cfg(unix)]
use reqwest::blocking::Client;
#[cfg(unix)]
use serde::Deserialize;
#[cfg(unix)]
use serde_json::Value;
#[cfg(unix)]
use std::collections::{BTreeMap, HashMap, VecDeque};
#[cfg(unix)]
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Condvar, Mutex,
};
#[cfg(unix)]
use std::time::{Duration, Instant, SystemTime};

#[cfg(unix)]
const TTL: Duration = Duration::from_secs(60);

#[cfg(unix)]
#[derive(Debug, Deserialize)]
struct PresignedChunkUrl {
    method: String,
    url: String,
    headers: BTreeMap<String, String>,
}

#[derive(Args)]
pub struct MountArgs {
    pub target: String,
    pub path: PathBuf,
    #[arg(long)]
    pub key: Option<String>,
    #[arg(long, default_value = "main")]
    pub branch: String,
    #[arg(long)]
    pub foreground: bool,
}

#[derive(Args)]
pub struct UnmountArgs {
    pub path: PathBuf,
}

#[cfg(unix)]
pub async fn mount(args: MountArgs) -> Result<()> {
    platform::mount(args).await
}

#[cfg(unix)]
pub fn unmount(args: UnmountArgs) -> Result<()> {
    platform::unmount(args)
}

#[cfg(not(unix))]
pub async fn mount(_args: MountArgs) -> Result<()> {
    bail!("Windows mount is not supported yet; ProjFS backend is planned.")
}

#[cfg(not(unix))]
pub fn unmount(_args: UnmountArgs) -> Result<()> {
    bail!("Windows mount is not supported yet; ProjFS backend is planned.")
}

pub fn vfs() -> Result<()> {
    let cfg = crate::config::load().unwrap_or_default();
    let (org, repo_name) = if let Some(ref root) = crate::repo::try_discover() {
        let local = crate::repo::load_config(root).unwrap_or_default();
        (
            local.org.or(cfg.current_org).unwrap_or_default(),
            local.repo.or(cfg.current_repo).unwrap_or_default(),
        )
    } else {
        (
            cfg.current_org.unwrap_or_default(),
            cfg.current_repo.unwrap_or_default(),
        )
    };

    println!("ClearMesh VFS — read-only FUSE mount");
    println!();
    if !org.is_empty() && !repo_name.is_empty() {
        println!("Mount current repo ({org}/{repo_name}):");
        println!("  mkdir ~/{repo_name}-vfs");
        println!("  clearmesh mount {org}/{repo_name} ~/{repo_name}-vfs");
        println!();
        println!("Encrypted repos require --key:");
        println!("  clearmesh mount {org}/{repo_name} ~/{repo_name}-vfs --key \"$PASSPHRASE\"");
    } else {
        println!("Mount a repository:");
        println!("  clearmesh mount ORG/REPO /mnt/point");
        println!("  clearmesh mount ORG/REPO /mnt/point --key \"$PASSPHRASE\"  # encrypted");
    }
    println!();
    println!("Unmount:");
    println!("  clearmesh unmount /mnt/point   # calls fusermount -u on Linux");
    println!("  umount /mnt/point              # macOS / fallback");
    println!();
    println!("Requirements:");
    println!("  Linux:  sudo apt install fuse3  (or dnf/pacman equivalent)");
    println!("  macOS:  https://osxfuse.github.io  (macFUSE)");
    Ok(())
}

#[cfg(unix)]
mod platform {
    use super::*;

    pub(super) async fn mount(args: MountArgs) -> Result<()> {
        let cfg = config::load()?;
        let token = cfg
            .token
            .clone()
            .filter(|token| !token.trim().is_empty())
            .context("login required before mounting; run clearmesh login")?;
        let (org, repo) = args
            .target
            .split_once('/')
            .context("mount target must be ORG/REPO")?;
        fs::create_dir_all(&args.path)
            .with_context(|| format!("create mount directory {}", args.path.display()))?;

        let api = ApiClient::from_config(&cfg);
        let repo_value = api.get(&format!("/v1/orgs/{org}/repos/{repo}")).await?;
        let encryption_mode = repo_value["repo"]["encryption_mode"]
            .as_str()
            .or_else(|| repo_value["encryption_mode"].as_str())
            .unwrap_or("required");
        let key = if encryption_mode == "none" {
            None
        } else {
            Some(demo_key_from_passphrase(
                args.key
                    .as_deref()
                    .context("repo is encrypted; pass --key <PASSPHRASE>")?,
            ))
        };
        let branch_value = api
            .get(&format!(
                "/v1/orgs/{org}/repos/{}/branches/{}",
                repo, args.branch
            ))
            .await?;
        let commit_id = branch_value["branch"]["head_commit_id"]
            .as_str()
            .context("branch has no commits")?
            .to_string();
        let tree = api
            .get(&format!(
                "/v1/orgs/{org}/repos/{repo}/commits/{commit_id}/tree"
            ))
            .await?;
        let commit = api
            .get(&format!("/v1/orgs/{org}/repos/{repo}/commits/{commit_id}"))
            .await?;
        let files = files_from_tree(&tree)?;
        let mtime = commit["commit"]["created_at"]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| SystemTime::from(value.with_timezone(&chrono::Utc)))
            .unwrap_or_else(SystemTime::now);

        let signals = block_shutdown_signals()?;
        let fs = ClearMeshFs::new(
            cfg.api.trim_end_matches('/').to_string(),
            token,
            org.to_string(),
            repo.to_string(),
            key,
            files,
            mtime,
        )?;
        let options = vec![
            MountOption::RO,
            MountOption::FSName(format!("clearmesh:{org}/{repo}@{}", args.branch)),
        ];
        let stats = fs.shared.clone();
        let session = fuser::spawn_mount2(fs, &args.path, &options)?;
        println!(
            "Mounted {org}/{repo}@{} at {}",
            args.branch,
            args.path.display()
        );
        println!("Files stream from Vault on demand.");
        println!("Press Ctrl+C to unmount.");
        wait_for_mount_shutdown(&session, signals.as_ref());
        drop(session);
        std::thread::sleep(Duration::from_millis(100));
        print_mount_stats(&stats);
        Ok(())
    }

    #[cfg(unix)]
    pub(super) fn unmount(args: UnmountArgs) -> Result<()> {
        let status = Command::new("fusermount3")
            .arg("-u")
            .arg(&args.path)
            .status();
        if matches!(status, Ok(status) if status.success()) {
            return Ok(());
        }
        let status = Command::new("umount")
            .arg(&args.path)
            .status()
            .context("run fusermount3 or umount")?;
        if !status.success() {
            bail!("failed to unmount {}", args.path.display());
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn block_shutdown_signals() -> Result<Option<libc::sigset_t>> {
        unsafe {
            let mut signals = std::mem::zeroed();
            if libc::sigemptyset(&mut signals) != 0
                || libc::sigaddset(&mut signals, libc::SIGINT) != 0
                || libc::sigaddset(&mut signals, libc::SIGTERM) != 0
                || libc::pthread_sigmask(libc::SIG_BLOCK, &signals, std::ptr::null_mut()) != 0
            {
                bail!("failed to install mount shutdown handler");
            }
            Ok(Some(signals))
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn block_shutdown_signals() -> Result<Option<libc::sigset_t>> {
        Ok(None)
    }

    #[cfg(target_os = "linux")]
    fn wait_for_mount_shutdown(session: &BackgroundSession, signals: Option<&libc::sigset_t>) {
        let Some(signals) = signals else {
            wait_for_mount_poll(session);
            return;
        };
        while !session.guard.is_finished() {
            let timeout = libc::timespec {
                tv_sec: 0,
                tv_nsec: 250_000_000,
            };
            let signal = unsafe { libc::sigtimedwait(signals, std::ptr::null_mut(), &timeout) };
            if signal == libc::SIGINT || signal == libc::SIGTERM {
                break;
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn wait_for_mount_shutdown(session: &BackgroundSession, _signals: Option<&libc::sigset_t>) {
        wait_for_mount_poll(session);
    }

    fn wait_for_mount_poll(session: &BackgroundSession) {
        while !session.guard.is_finished() {
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    #[derive(Clone)]
    struct Node {
        ino: u64,
        parent: u64,
        kind: FileType,
        file: Option<FileEntry>,
        children: BTreeMap<OsString, u64>,
    }

    struct ClearMeshFs {
        shared: Arc<MountShared>,
        nodes: HashMap<u64, Node>,
        mtime: SystemTime,
    }

    struct MountShared {
        org: String,
        repo: String,
        base: String,
        token: String,
        key: Option<[u8; 32]>,
        cache_dir: PathBuf,
        decrypted: Mutex<MemoryCache>,
        in_flight: Mutex<HashMap<String, Arc<InFlight>>>,
        sequential_reads: Mutex<HashMap<String, u64>>,
        fetch_slots: FetchSlots,
        stats: MountStats,
        client: Client,
        readahead_chunks: u32,
    }

    struct InFlight {
        result: Mutex<Option<std::result::Result<Vec<u8>, i32>>>,
        ready: Condvar,
    }

    struct FetchSlots {
        available: Mutex<usize>,
        ready: Condvar,
    }

    struct FetchPermit<'a> {
        slots: &'a FetchSlots,
    }

    #[derive(Default)]
    struct MountStats {
        bytes_served: AtomicU64,
        chunks_fetched: AtomicU64,
        memory_hits: AtomicU64,
        disk_hits: AtomicU64,
        disk_misses: AtomicU64,
        decrypt_failures: AtomicU64,
        fetch_ms_total: AtomicU64,
    }

    fn print_mount_stats(shared: &MountShared) {
        let fetched = shared.stats.chunks_fetched.load(Ordering::Relaxed);
        let total_ms = shared.stats.fetch_ms_total.load(Ordering::Relaxed);
        let avg_ms = if fetched == 0 { 0 } else { total_ms / fetched };
        let memory_hits = shared.stats.memory_hits.load(Ordering::Relaxed);
        let disk_hits = shared.stats.disk_hits.load(Ordering::Relaxed);
        let disk_misses = shared.stats.disk_misses.load(Ordering::Relaxed);
        eprintln!("ClearMesh mount stats:");
        eprintln!(
            "  bytes served: {}",
            shared.stats.bytes_served.load(Ordering::Relaxed)
        );
        eprintln!("  chunks fetched: {fetched}");
        if shared.key.is_some() {
            eprintln!("  encrypted cache hits: {disk_hits}");
            eprintln!("  encrypted cache misses: {disk_misses}");
            eprintln!("  decrypted memory hits: {memory_hits}");
        } else {
            eprintln!("  object cache hits: {disk_hits}");
            eprintln!("  object cache misses: {disk_misses}");
            eprintln!("  object memory hits: {memory_hits}");
        }
        eprintln!(
            "  decrypt failures: {}",
            shared.stats.decrypt_failures.load(Ordering::Relaxed)
        );
        eprintln!("  average fetch: {avg_ms} ms");
    }

    impl ClearMeshFs {
        fn new(
            base: String,
            token: String,
            org: String,
            repo: String,
            key: Option<[u8; 32]>,
            files: Vec<FileEntry>,
            mtime: SystemTime,
        ) -> Result<Self> {
            let cache_dir = dirs::cache_dir()
                .or_else(|| dirs::home_dir().map(|home| home.join(".cache")))
                .context("cache directory not found")?
                .join("clearmesh/objects");
            let nodes = build_tree(files)?;
            let memory_cache_max = env_usize("CLEARMESH_MOUNT_MEMORY_CACHE_MB", 256) * 1024 * 1024;
            let fetch_concurrency = env_usize("CLEARMESH_MOUNT_FETCH_CONCURRENCY", 8).max(1);
            let readahead_chunks = env_usize("CLEARMESH_MOUNT_READAHEAD_CHUNKS", 4) as u32;
            Ok(Self {
                shared: Arc::new(MountShared {
                    org,
                    repo,
                    base,
                    token,
                    key,
                    cache_dir,
                    decrypted: Mutex::new(MemoryCache::new(memory_cache_max)),
                    in_flight: Mutex::new(HashMap::new()),
                    sequential_reads: Mutex::new(HashMap::new()),
                    fetch_slots: FetchSlots::new(fetch_concurrency),
                    stats: MountStats::default(),
                    client: Client::new(),
                    readahead_chunks,
                }),
                nodes,
                mtime,
            })
        }

        fn attr(&self, node: &Node) -> FileAttr {
            let size = node.file.as_ref().map(|file| file.size).unwrap_or(0);
            FileAttr {
                ino: node.ino,
                size,
                blocks: size.div_ceil(512),
                atime: self.mtime,
                mtime: self.mtime,
                ctime: self.mtime,
                crtime: self.mtime,
                kind: node.kind,
                perm: if node.kind == FileType::Directory {
                    0o555
                } else {
                    0o444
                },
                nlink: if node.kind == FileType::Directory {
                    2
                } else {
                    1
                },
                uid: unsafe { libc::getuid() },
                gid: unsafe { libc::getgid() },
                rdev: 0,
                flags: 0,
                blksize: 4096,
            }
        }

        fn read_file(
            &self,
            file: &FileEntry,
            offset: i64,
            size: u32,
        ) -> std::result::Result<Vec<u8>, i32> {
            if offset < 0 {
                return Ok(Vec::new());
            }

            let start = offset as u64;
            if start >= file.size || size == 0 {
                return Ok(Vec::new());
            }

            let end = (start + size as u64).min(file.size);
            if file.chunks.is_empty() {
                eprintln!(
                    "cannot read {}; file has size {} but no chunks",
                    file.path, file.size
                );
                return Err(EIO);
            }

            let encrypted_overhead = if self.shared.key.is_some() {
                28u64
            } else {
                0u64
            };

            let mut chunks: Vec<&ChunkRef> = file.chunks.iter().collect();
            chunks.sort_by_key(|chunk| chunk.index);

            let mut out = Vec::with_capacity((end - start) as usize);
            let mut chunk_start = 0u64;
            let mut first_index: Option<u64> = None;
            let mut next_index = 0u64;

            for chunk in chunks {
                if chunk_start >= file.size || chunk_start >= end {
                    break;
                }

                let stored_size = chunk.size;
                let guessed_plaintext_size = stored_size.saturating_sub(encrypted_overhead);
                let remaining_file_bytes = file.size.saturating_sub(chunk_start);
                let chunk_len = guessed_plaintext_size.min(remaining_file_bytes);
                let chunk_end = chunk_start.saturating_add(chunk_len);

                if chunk_end <= start {
                    chunk_start = chunk_end;
                    continue;
                }

                if chunk_start >= end {
                    break;
                }

                let plaintext = self.chunk_plaintext(chunk)?;

                let read_start = start.max(chunk_start).saturating_sub(chunk_start);
                let read_end = end.min(chunk_end).saturating_sub(chunk_start);

                let slice_start = (read_start as usize).min(plaintext.len());
                let slice_end = (read_end as usize).min(plaintext.len());

                if slice_start < slice_end {
                    out.extend_from_slice(&plaintext[slice_start..slice_end]);
                }

                first_index.get_or_insert(chunk.index as u64);
                next_index = chunk.index as u64 + 1;
                chunk_start = chunk_end;
            }

            self.shared
                .stats
                .bytes_served
                .fetch_add(out.len() as u64, Ordering::Relaxed);

            if out.len() != (end - start) as usize {
                eprintln!(
                    "short VFS read for {} at offset {}: wanted {} bytes, served {} bytes",
                    file.path,
                    start,
                    end - start,
                    out.len()
                );
                return Err(EIO);
            }

            if let Some(first) = first_index {
                self.maybe_readahead(file, first, next_index);
            }

            Ok(out)
        }

        fn chunk_plaintext(&self, chunk: &ChunkRef) -> std::result::Result<Vec<u8>, i32> {
            if let Some(bytes) = self
                .shared
                .decrypted
                .lock()
                .map_err(|_| EIO)?
                .get(&chunk.hash)
            {
                self.shared
                    .stats
                    .memory_hits
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(bytes);
            }
            let stored = self.encrypted_chunk(chunk)?;
            let plaintext = if let Some(key) = &self.shared.key {
                decrypt_chunk(key, &stored).map_err(|_| {
                    self.shared
                        .stats
                        .decrypt_failures
                        .fetch_add(1, Ordering::Relaxed);
                    eprintln!("decryption failed; check key");
                    EIO
                })?
            } else {
                stored
            };
            self.shared
                .decrypted
                .lock()
                .map_err(|_| EIO)?
                .insert(chunk.hash.clone(), plaintext.clone());
            Ok(plaintext)
        }

        fn encrypted_chunk(&self, chunk: &ChunkRef) -> std::result::Result<Vec<u8>, i32> {
            let path = cache_path(&self.shared.cache_dir, &chunk.hash);
            if let Ok(bytes) = fs::read(&path) {
                if hash_bytes(&bytes) == chunk.hash {
                    self.shared.stats.disk_hits.fetch_add(1, Ordering::Relaxed);
                    return Ok(bytes);
                }
                // Hash mismatch: cache file is corrupt; delete it and refetch.
                eprintln!(
                    "cached chunk {} has wrong hash; deleting and refetching",
                    chunk.hash
                );
                let _ = fs::remove_file(&path);
            }
            self.shared
                .stats
                .disk_misses
                .fetch_add(1, Ordering::Relaxed);
            self.fetch_encrypted_coalesced(chunk, path)
        }

        fn fetch_encrypted_coalesced(
            &self,
            chunk: &ChunkRef,
            path: PathBuf,
        ) -> std::result::Result<Vec<u8>, i32> {
            let (entry, owner) = {
                let mut in_flight = self.shared.in_flight.lock().map_err(|_| EIO)?;
                if let Some(entry) = in_flight.get(&chunk.hash) {
                    (entry.clone(), false)
                } else {
                    let entry = Arc::new(InFlight::new());
                    in_flight.insert(chunk.hash.clone(), entry.clone());
                    (entry, true)
                }
            };
            if owner {
                let result = self.fetch_encrypted_uncached(chunk, &path);
                *entry.result.lock().map_err(|_| EIO)? = Some(result.clone());
                entry.ready.notify_all();
                if let Ok(mut in_flight) = self.shared.in_flight.lock() {
                    in_flight.remove(&chunk.hash);
                }
                result
            } else {
                let mut guard = entry.result.lock().map_err(|_| EIO)?;
                while guard.is_none() {
                    guard = entry.ready.wait(guard).map_err(|_| EIO)?;
                }
                guard.as_ref().unwrap().clone()
            }
        }

        fn fetch_encrypted_uncached(
            &self,
            chunk: &ChunkRef,
            path: &Path,
        ) -> std::result::Result<Vec<u8>, i32> {
            let _permit = self.shared.fetch_slots.acquire().map_err(|_| EIO)?;
            let presign_url = format!(
                "{}/v1/orgs/{}/repos/{}/vault/chunks/{}/presign-get",
                self.shared.base, self.shared.org, self.shared.repo, chunk.hash
            );
            let started = Instant::now();
            let response = self
                .shared
                .client
                .post(presign_url)
                .bearer_auth(&self.shared.token)
                .json(&serde_json::json!({}))
                .send()
                .map_err(|_| EIO)?;
            if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
                return Err(EACCES);
            }
            if !response.status().is_success() {
                eprintln!("direct Vault download failed: presign failed");
                return Err(EIO);
            }
            let signed: PresignedChunkUrl = response.json().map_err(|_| EIO)?;
            if signed.method != "GET" {
                eprintln!("direct Vault download failed: presign method mismatch");
                return Err(EIO);
            }
            let mut request = self.shared.client.get(&signed.url);
            for (name, value) in &signed.headers {
                request = request.header(name.as_str(), value.as_str());
            }
            let response = request.send().map_err(|_| EIO)?;
            self.shared
                .stats
                .fetch_ms_total
                .fetch_add(started.elapsed().as_millis() as u64, Ordering::Relaxed);
            if !response.status().is_success() {
                eprintln!(
                    "direct Vault download failed: chunk unavailable {}",
                    chunk.hash
                );
                return Err(EIO);
            }
            let bytes = response.bytes().map_err(|_| EIO)?.to_vec();
            if hash_bytes(&bytes) != chunk.hash {
                eprintln!("chunk hash mismatch {}", chunk.hash);
                return Err(EIO);
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|_| EIO)?;
            }
            fs::write(path, &bytes).map_err(|_| EIO)?;
            self.shared
                .stats
                .chunks_fetched
                .fetch_add(1, Ordering::Relaxed);
            let usage_url = format!(
                "{}/v1/orgs/{}/repos/{}/usage/chunk-download",
                self.shared.base, self.shared.org, self.shared.repo
            );
            let _ = self
                .shared
                .client
                .post(usage_url)
                .bearer_auth(&self.shared.token)
                .json(&serde_json::json!({
                    "hash": chunk.hash,
                    "bytes": bytes.len(),
                    "source": "direct",
                }))
                .send();
            Ok(bytes)
        }

        fn maybe_readahead(&self, file: &FileEntry, first_index: u64, next_index: u64) {
            let sequential = self
                .shared
                .sequential_reads
                .lock()
                .map(|mut reads| {
                    let expected = reads.get(&file.path).copied();
                    reads.insert(file.path.clone(), next_index);
                    expected
                        .map(|expected| first_index <= expected + 1 && next_index >= expected)
                        .unwrap_or(first_index == 0)
                })
                .unwrap_or(false);
            if sequential {
                self.readahead(file, next_index);
            }
        }

        fn readahead(&self, file: &FileEntry, start_index: u64) {
            if self.shared.readahead_chunks == 0 {
                return;
            }
            let shared = self.shared.clone();
            let chunks: Vec<ChunkRef> = file
                .chunks
                .iter()
                .filter(|chunk| {
                    chunk.index as u64 >= start_index
                        && (chunk.index as u64) < start_index + shared.readahead_chunks as u64
                        && shared
                            .decrypted
                            .lock()
                            .map(|cache| !cache.entries.contains_key(&chunk.hash))
                            .unwrap_or(false)
                })
                .cloned()
                .collect();
            for chunk in chunks {
                let fs = Self {
                    shared: shared.clone(),
                    nodes: HashMap::new(),
                    mtime: self.mtime,
                };
                std::thread::spawn(move || {
                    let _ = fs.chunk_plaintext(&chunk);
                });
            }
        }
    }

    impl Filesystem for ClearMeshFs {
        fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
            let Some(parent_node) = self.nodes.get(&parent) else {
                reply.error(ENOENT);
                return;
            };
            let Some(ino) = parent_node.children.get(name) else {
                reply.error(ENOENT);
                return;
            };
            let node = &self.nodes[ino];
            reply.entry(&TTL, &self.attr(node), 0);
        }

        fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
            match self.nodes.get(&ino) {
                Some(node) => reply.attr(&TTL, &self.attr(node)),
                None => reply.error(ENOENT),
            }
        }

        fn readdir(
            &mut self,
            _req: &Request<'_>,
            ino: u64,
            _fh: u64,
            offset: i64,
            mut reply: ReplyDirectory,
        ) {
            let Some(node) = self.nodes.get(&ino) else {
                reply.error(ENOENT);
                return;
            };
            if node.kind != FileType::Directory {
                reply.error(ENOENT);
                return;
            }
            let mut entries = vec![
                (node.ino, FileType::Directory, OsString::from(".")),
                (node.parent, FileType::Directory, OsString::from("..")),
            ];
            for (name, child_ino) in &node.children {
                entries.push((*child_ino, self.nodes[child_ino].kind, name.clone()));
            }
            for (i, (entry_ino, kind, name)) in
                entries.into_iter().enumerate().skip(offset as usize)
            {
                if reply.add(entry_ino, (i + 1) as i64, kind, name) {
                    break;
                }
            }
            reply.ok();
        }

        fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
            let Some(node) = self.nodes.get(&ino) else {
                reply.error(ENOENT);
                return;
            };
            if node.kind != FileType::RegularFile {
                reply.error(ENOENT);
                return;
            }
            if flags & O_ACCMODE != O_RDONLY {
                reply.error(EROFS);
                return;
            }
            reply.opened(0, 0);
        }

        fn read(
            &mut self,
            _req: &Request<'_>,
            ino: u64,
            _fh: u64,
            offset: i64,
            size: u32,
            _flags: i32,
            _lock_owner: Option<u64>,
            reply: ReplyData,
        ) {
            let Some(node) = self.nodes.get(&ino) else {
                reply.error(ENOENT);
                return;
            };
            let Some(file) = node.file.as_ref() else {
                reply.error(ENOENT);
                return;
            };
            match self.read_file(file, offset, size) {
                Ok(bytes) => reply.data(&bytes),
                Err(code) => reply.error(code),
            }
        }
    }

    struct MemoryCache {
        entries: HashMap<String, Vec<u8>>,
        order: VecDeque<String>,
        bytes: usize,
        max_bytes: usize,
    }

    impl MemoryCache {
        fn new(max_bytes: usize) -> Self {
            Self {
                entries: HashMap::new(),
                order: VecDeque::new(),
                bytes: 0,
                max_bytes,
            }
        }

        fn get(&mut self, hash: &str) -> Option<Vec<u8>> {
            let bytes = self.entries.get(hash).cloned()?;
            self.order.retain(|entry| entry != hash);
            self.order.push_back(hash.to_string());
            Some(bytes)
        }

        fn insert(&mut self, hash: String, bytes: Vec<u8>) {
            if self.entries.contains_key(&hash) {
                return;
            }
            self.bytes += bytes.len();
            self.order.push_back(hash.clone());
            self.entries.insert(hash, bytes);
            while self.bytes > self.max_bytes {
                let Some(old) = self.order.pop_front() else {
                    break;
                };
                if let Some(bytes) = self.entries.remove(&old) {
                    self.bytes = self.bytes.saturating_sub(bytes.len());
                }
            }
        }
    }

    impl InFlight {
        fn new() -> Self {
            Self {
                result: Mutex::new(None),
                ready: Condvar::new(),
            }
        }
    }

    impl FetchSlots {
        fn new(max: usize) -> Self {
            Self {
                available: Mutex::new(max),
                ready: Condvar::new(),
            }
        }

        fn acquire(&self) -> std::result::Result<FetchPermit<'_>, i32> {
            let mut available = self.available.lock().map_err(|_| EIO)?;
            while *available == 0 {
                available = self.ready.wait(available).map_err(|_| EIO)?;
            }
            *available -= 1;
            Ok(FetchPermit { slots: self })
        }
    }

    impl Drop for FetchPermit<'_> {
        fn drop(&mut self) {
            if let Ok(mut available) = self.slots.available.lock() {
                *available += 1;
                self.slots.ready.notify_one();
            }
        }
    }

    fn env_usize(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    fn build_tree(files: Vec<FileEntry>) -> Result<HashMap<u64, Node>> {
        let mut nodes = HashMap::new();
        nodes.insert(
            1,
            Node {
                ino: 1,
                parent: 1,
                kind: FileType::Directory,
                file: None,
                children: BTreeMap::new(),
            },
        );
        let mut next_ino = 2u64;
        for file in files {
            let parts: Vec<&str> = file
                .path
                .split('/')
                .filter(|part| !part.is_empty())
                .collect();
            if parts.is_empty() {
                continue;
            }
            let mut parent = 1u64;
            for dir in &parts[..parts.len() - 1] {
                let name = OsString::from(dir);
                if let Some(ino) = nodes[&parent].children.get(&name).copied() {
                    parent = ino;
                    continue;
                }
                let ino = next_ino;
                next_ino += 1;
                nodes
                    .get_mut(&parent)
                    .unwrap()
                    .children
                    .insert(name.clone(), ino);
                nodes.insert(
                    ino,
                    Node {
                        ino,
                        parent,
                        kind: FileType::Directory,
                        file: None,
                        children: BTreeMap::new(),
                    },
                );
                parent = ino;
            }
            let name = OsString::from(parts[parts.len() - 1]);
            let ino = next_ino;
            next_ino += 1;
            nodes
                .get_mut(&parent)
                .unwrap()
                .children
                .insert(name.clone(), ino);
            nodes.insert(
                ino,
                Node {
                    ino,
                    parent,
                    kind: FileType::RegularFile,
                    file: Some(file),
                    children: BTreeMap::new(),
                },
            );
        }
        Ok(nodes)
    }

    fn cache_path(root: &Path, hash: &str) -> PathBuf {
        let prefix = hash.get(..2).unwrap_or("xx");
        root.join(prefix).join(hash)
    }

    fn files_from_tree(tree: &Value) -> Result<Vec<FileEntry>> {
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
                let size = file["size"].as_i64().unwrap_or_default() as u64;
                let chunk_size = if chunks.len() > 1 {
                    size.div_ceil(chunks.len() as u64) as usize
                } else {
                    size.max(1) as usize
                };

                Ok(FileEntry {
                    path: path.to_string(),
                    size,
                    mode: 0o100444,
                    chunks,
                    chunk_size,
                })
            })
            .collect()
    }
}
