use crate::commands::commit::require_local_repo;
use crate::repo as local_repo;
use anyhow::{bail, Context, Result};
use clap::Args;
use clearmesh_core::{decrypt_chunk, demo_key_from_passphrase, Commit, FileEntry};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct MergeArgs {
    pub branch: String,
    #[arg(long)]
    pub key: Option<String>,
}

#[derive(Args)]
pub struct CheckoutArgs {
    #[arg(long, conflicts_with = "theirs")]
    pub ours: bool,
    #[arg(long, conflicts_with = "ours")]
    pub theirs: bool,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeState {
    pub branch: String,
    pub ours: String,
    pub theirs: String,
    pub base: Option<String>,
    pub conflicts: Vec<ConflictState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictState {
    pub path: String,
    pub resolved: bool,
}

pub fn merge(args: MergeArgs) -> Result<()> {
    let (root, _) = require_local_repo()?;
    ensure_no_merge(&root)?;
    let ours = local_repo::current_commit_id(&root)?.context("current branch has no commits")?;
    let theirs = branch_head(&root, &args.branch)?.context("merge branch has no commits")?;

    if is_ancestor(&root, &theirs, &ours)? {
        println!("Already up to date");
        return Ok(());
    }

    if is_ancestor(&root, &ours, &theirs)? {
        local_repo::update_head(&root, &theirs)?;
        println!("Fast-forwarded to {theirs}");
        return Ok(());
    }

    let base = merge_base(&root, &ours, &theirs)?;
    let ours_commit = local_repo::read_commit(&root, &ours)?;
    let theirs_commit = local_repo::read_commit(&root, &theirs)?;
    let base_commit = base
        .as_deref()
        .map(|id| local_repo::read_commit(&root, id))
        .transpose()?;

    let mut conflicts = Vec::new();
    let mut paths = HashSet::new();
    let ours_files = file_map(&ours_commit);
    let theirs_files = file_map(&theirs_commit);
    let base_files = base_commit.as_ref().map(file_map).unwrap_or_default();
    paths.extend(ours_files.keys().cloned());
    paths.extend(theirs_files.keys().cloned());
    paths.extend(base_files.keys().cloned());

    for path in paths {
        let base_file = base_files.get(path.as_str()).copied();
        let ours_file = ours_files.get(path.as_str()).copied();
        let theirs_file = theirs_files.get(path.as_str()).copied();
        let ours_changed = ours_file != base_file;
        let theirs_changed = theirs_file != base_file;

        if ours_changed && theirs_changed && ours_file != theirs_file {
            write_conflict_files(
                &root,
                path.as_str(),
                base_file,
                ours_file,
                theirs_file,
                &args.key,
            )?;
            conflicts.push(ConflictState {
                path,
                resolved: false,
            });
        } else if theirs_changed && !ours_changed {
            apply_entry(&root, path.as_str(), theirs_file, &args.key)?;
        }
    }

    let state = MergeState {
        branch: args.branch,
        ours,
        theirs,
        base,
        conflicts,
    };
    save_state(&root, &state)?;
    if state.conflicts.is_empty() {
        println!(
            "Merged cleanly; run clearmesh commit -m \"merge {}\"",
            state.branch
        );
    } else {
        println!("Merge has conflicts; run clearmesh conflicts");
    }
    Ok(())
}

pub fn conflicts() -> Result<()> {
    let (root, _) = require_local_repo()?;
    let state = load_state(&root)?;
    for conflict in state.conflicts {
        let status = if conflict.resolved {
            "resolved"
        } else {
            "unresolved"
        };
        println!("{status}\t{}", conflict.path);
    }
    Ok(())
}

pub fn checkout(args: CheckoutArgs) -> Result<()> {
    let (root, _) = require_local_repo()?;
    let path = normalize_path(&args.path);
    let src = if args.theirs {
        root.join(format!("{path}.THEIRS"))
    } else {
        root.join(format!("{path}.OURS"))
    };
    if !src.exists() {
        bail!("conflict side file not found for {path}");
    }
    fs::copy(&src, root.join(&path))?;
    println!("Checked out {path}");
    Ok(())
}

pub fn resolve(path: PathBuf) -> Result<()> {
    let (root, _) = require_local_repo()?;
    let path = normalize_path(&path);
    let mut state = load_state(&root)?;
    let Some(conflict) = state.conflicts.iter_mut().find(|c| c.path == path) else {
        bail!("path is not in conflict: {path}");
    };
    conflict.resolved = true;
    remove_side_file(&root, &path, "OURS")?;
    remove_side_file(&root, &path, "THEIRS")?;
    remove_side_file(&root, &path, "BASE")?;
    save_state(&root, &state)?;
    println!("Resolved {path}");
    Ok(())
}

pub fn pending_merge(root: &Path) -> Result<Option<MergeState>> {
    let path = state_path(root);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

pub fn finish_merge(root: &Path) -> Result<()> {
    let Some(state) = pending_merge(root)? else {
        return Ok(());
    };
    if state.conflicts.iter().any(|c| !c.resolved) {
        bail!("merge has unresolved conflicts; run clearmesh conflicts");
    }
    fs::remove_file(state_path(root))?;
    Ok(())
}

fn branch_head(root: &Path, branch: &str) -> Result<Option<String>> {
    let path = root.join(".clearmesh/refs/heads").join(branch);
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)?.trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn file_map(commit: &Commit) -> HashMap<String, &FileEntry> {
    commit
        .files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect()
}

fn apply_entry(
    root: &Path,
    path: &str,
    file: Option<&FileEntry>,
    key: &Option<String>,
) -> Result<()> {
    let Some(file) = file else {
        let out = root.join(path);
        if out.exists() {
            fs::remove_file(out)?;
        }
        return Ok(());
    };
    let bytes = materialize(root, file, key)?;
    let out = root.join(&file.path);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out, bytes)?;
    Ok(())
}

fn write_conflict_files(
    root: &Path,
    path: &str,
    base: Option<&FileEntry>,
    ours: Option<&FileEntry>,
    theirs: Option<&FileEntry>,
    key: &Option<String>,
) -> Result<()> {
    if let Some(file) = ours {
        write_side(root, path, "OURS", materialize(root, file, key)?)?;
    }
    if let Some(file) = theirs {
        write_side(root, path, "THEIRS", materialize(root, file, key)?)?;
    }
    if let Some(file) = base {
        write_side(root, path, "BASE", materialize(root, file, key)?)?;
    }
    Ok(())
}

fn write_side(root: &Path, path: &str, side: &str, bytes: Vec<u8>) -> Result<()> {
    let out = root.join(format!("{path}.{side}"));
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out, bytes)?;
    Ok(())
}

fn materialize(root: &Path, file: &FileEntry, key: &Option<String>) -> Result<Vec<u8>> {
    let key = key
        .as_deref()
        .context("merge needs --key to materialize historical file contents")?;
    let key = demo_key_from_passphrase(key);
    let mut bytes = Vec::new();
    for chunk in &file.chunks {
        let encrypted = local_repo::read_object(root, &chunk.hash)?;
        bytes.extend(decrypt_chunk(&key, &encrypted)?);
    }
    Ok(bytes)
}

fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    if ancestor == descendant {
        return Ok(true);
    }
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([descendant.to_string()]);
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let commit = local_repo::read_commit(root, &id)?;
        for parent in commit.parent_ids {
            if parent == ancestor {
                return Ok(true);
            }
            queue.push_back(parent);
        }
    }
    Ok(false)
}

fn merge_base(root: &Path, ours: &str, theirs: &str) -> Result<Option<String>> {
    let ours_ancestors = ancestors(root, ours)?;
    let mut queue = VecDeque::from([theirs.to_string()]);
    let mut seen = HashSet::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if ours_ancestors.contains(&id) {
            return Ok(Some(id));
        }
        let commit = local_repo::read_commit(root, &id)?;
        queue.extend(commit.parent_ids);
    }
    Ok(None)
}

fn ancestors(root: &Path, head: &str) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    let mut queue = VecDeque::from([head.to_string()]);
    while let Some(id) = queue.pop_front() {
        if !out.insert(id.clone()) {
            continue;
        }
        let commit = local_repo::read_commit(root, &id)?;
        queue.extend(commit.parent_ids);
    }
    Ok(out)
}

fn state_path(root: &Path) -> PathBuf {
    root.join(".clearmesh/MERGE_STATE.json")
}

fn save_state(root: &Path, state: &MergeState) -> Result<()> {
    fs::write(state_path(root), serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

fn load_state(root: &Path) -> Result<MergeState> {
    serde_json::from_slice(&fs::read(state_path(root))?).context("no merge in progress")
}

fn ensure_no_merge(root: &Path) -> Result<()> {
    if state_path(root).exists() {
        bail!("merge already in progress");
    }
    Ok(())
}

fn remove_side_file(root: &Path, path: &str, side: &str) -> Result<()> {
    let side_path = root.join(format!("{path}.{side}"));
    if side_path.exists() {
        fs::remove_file(side_path)?;
    }
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches("./")
        .replace('\\', "/")
}
