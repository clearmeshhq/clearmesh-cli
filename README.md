# ClearMesh CLI

A command-line tool for versioning large files and binary folders using a Git-like workflow backed by chunked object storage.

---

## What ClearMesh CLI is for

ClearMesh is designed for working with large files that Git handles poorly: model weights, datasets, rendered assets, binary archives, media folders. It chunks files locally, tracks them in commits, and stores chunks in a Vault backed by S3-compatible object storage.

You can commit and push from a local folder, clone to a new machine, sync updates, or mount a repository as a read-only filesystem and stream files on demand without downloading everything first.

## What it is not

ClearMesh is not a replacement for Git source control. It tracks file versions and branches but does not handle code diffs, pull requests, or text merges. It is complementary to Git, not a substitute.

---

## Install

**Linux and macOS:**

```bash
curl -fsSL https://clearmesh.net/install.sh | bash
```

The install script places the binary at `~/.local/bin/clearmesh` and updates your PATH if needed.

**Build from source:**

```bash
cargo build --release
install -m 0755 target/release/clearmesh-cli ~/.local/bin/clearmesh
```

Requires Rust 1.70 or later.

**Mount support:**

On Linux, `mount` requires FUSE3:

```bash
sudo apt install -y fuse3        # Debian/Ubuntu
sudo dnf install -y fuse3        # Fedora/RHEL
```

On macOS, macFUSE is required if mount is supported on your version.

---

## Login

```bash
clearmesh login
```

Opens a browser flow to approve the CLI for your account. All account management (signup, password, 2FA) stays in the web console.

```bash
clearmesh auth me         # show which account is logged in
clearmesh logout
```

---

## Set API endpoint

By default the CLI targets `https://api.clearmesh.net`. To point at a self-hosted instance or a staging environment:

```bash
clearmesh config set-api https://api.clearmesh.net
clearmesh config show
```

Run `clearmesh doctor` after changing the endpoint to verify connectivity.

---

## Organizations

Your repositories live inside an organization. Create one if you do not have one yet.

```bash
clearmesh org list
clearmesh org create --slug acme --name "Acme"
clearmesh org use acme
```

`org use` sets the default org for subsequent commands that take an `ORG/REPO` argument.

---

## Repositories

```bash
clearmesh repo list
clearmesh repo show
```

Create a private encrypted repo (the default):

```bash
clearmesh repo create --slug dataset --name "Dataset" --visibility private --encryption required
```

Create a public unencrypted repo:

```bash
clearmesh repo create --slug open-data --name "Open Data" --visibility public --encryption none
```

Set the active repository for the current shell session:

```bash
clearmesh repo use acme/dataset
```

---

## Work in a local folder

Initialize a folder to track it as a ClearMesh repository:

```bash
mkdir dataset
cd dataset
clearmesh init
clearmesh repo use acme/dataset
```

Check what has changed:

```bash
clearmesh status
```

---

## Commit and push

**Encrypted repository:**

```bash
echo "hello ClearMesh" > README.md
clearmesh status
clearmesh commit --message "initial commit" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
```

**Unencrypted repository:**

```bash
clearmesh commit --message "initial commit"
clearmesh push
```

`-m` is short for `--message`.

On push, unchanged chunks from previous commits are reused automatically. Only new or modified chunks are uploaded.

---

## Clone and sync

**Encrypted:**

```bash
clearmesh clone acme/dataset ./dataset-copy --key "$PASSPHRASE"
cd dataset-copy
clearmesh sync --key "$PASSPHRASE"
```

**Unencrypted/public:**

```bash
clearmesh clone acme/open-data ./open-data-copy
cd open-data-copy
clearmesh sync
```

`sync` pulls the latest commit on the current branch and downloads any new chunks.

---

## Branches and switching

```bash
clearmesh branch list
clearmesh branch create experiment
clearmesh branch switch experiment
clearmesh branch switch main
clearmesh branch delete experiment
```

View commit history on the current branch:

```bash
clearmesh log
```

---

## Merge and conflicts

Merge a branch into the current branch:

```bash
clearmesh merge experiment --key "$PASSPHRASE"
```

If there are conflicts:

```bash
clearmesh conflicts
```

For each conflicting file, either pick a side or edit manually:

```bash
# Accept one side automatically
clearmesh checkout path/to/file --ours
clearmesh checkout path/to/file --theirs

# Or edit the file manually, then mark it resolved
clearmesh resolve path/to/file
```

Once all conflicts are resolved, commit and push:

```bash
clearmesh commit --message "merge experiment" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
```

---

## Mounting repositories

Mount gives you a read-only view of a repository at a local path. Files appear immediately but chunks are fetched from the Vault as they are read. You do not need to download the whole repository first.

```bash
mkdir -p ./mnt
clearmesh mount acme/dataset ./mnt --key "$PASSPHRASE"
```

Unencrypted/public repo:

```bash
clearmesh mount acme/open-data ./mnt
```

Mount a specific branch (default is `main`):

```bash
clearmesh mount acme/dataset ./mnt --branch experiment --key "$PASSPHRASE"
```

Unmount:

```bash
clearmesh unmount ./mnt
```

If the mount process is killed and the path is stuck, unmount manually:

```bash
# Linux
fusermount -u ./mnt

# macOS
umount ./mnt
```

Run in the foreground for debugging (logs to stdout instead of daemonizing):

```bash
clearmesh mount acme/dataset ./mnt --foreground --key "$PASSPHRASE"
```

**Notes:**

- Mount is read-only. You cannot write files through the mount path.
- The first read of a file fetches the relevant chunks from the Vault. Subsequent reads can be faster if those chunks are already cached locally.
- The encrypted chunk cache stores ciphertext on disk. Plaintext is only in memory while serving a read.
- Mounting does not download the full repository. If a program reads the whole file, ClearMesh will eventually fetch all chunks for that file.

---

## Encrypted repositories

Encrypted repositories require a passphrase (`--key`) for any operation that reads or writes file content: `commit`, `push`, `clone`, `sync`, `merge`, and `mount`.

```bash
clearmesh commit -m "update" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
clearmesh clone acme/dataset ./copy --key "$PASSPHRASE"
clearmesh sync --key "$PASSPHRASE"
clearmesh mount acme/dataset ./mnt --key "$PASSPHRASE"
```

The passphrase never leaves your machine. Losing it means encrypted file contents cannot be recovered — ClearMesh has no way to decrypt them for you.

---

## Unencrypted and public repositories

Public unencrypted repositories can be cloned, synced, and mounted without authentication or a key:

```bash
clearmesh clone acme/open-data ./open-data-copy
clearmesh sync
clearmesh mount acme/open-data ./mnt
```

Do not store private data in public or unencrypted repositories.

---

## Large file behavior and caching

ClearMesh stores files as chunks, not whole copies per commit. When you push a large file that has not changed since the last commit, the new commit references the same chunk IDs. Only new or modified chunks are uploaded.

On clone and sync, chunks are downloaded and files are reconstructed locally.

On mount, file ranges are streamed through the VFS. Chunks that have already been fetched are cached locally and not re-downloaded.

The chunk existence check during push was previously slow for repositories with many reused chunks. This has been optimized and now completes in milliseconds for typical workloads.

The mount filesystem has been tested with real GGUF model files, including random reads, boundary reads, and loading via llama.cpp. Full SHA-256 of a mounted file matched the local copy.

---

## Common workflows

**Start a new encrypted dataset:**

```bash
clearmesh org use acme
clearmesh repo create --slug weights --name "Model Weights" --visibility private --encryption required
mkdir weights && cd weights
clearmesh init
clearmesh repo use acme/weights
cp /path/to/model.gguf .
clearmesh commit -m "add model" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
```

**Load a model without cloning:**

```bash
mkdir -p ./mnt
clearmesh mount acme/weights ./mnt --key "$PASSPHRASE"
./llama.cpp/main -m ./mnt/model.gguf ...
clearmesh unmount ./mnt
```

**Update a file and push:**

```bash
cp /path/to/model-v2.gguf .
clearmesh status
clearmesh commit -m "update model to v2" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
```

**Pull updates on another machine:**

```bash
cd weights
clearmesh sync --key "$PASSPHRASE"
```

---

## Command reference

| Command | Purpose |
|---|---|
| `login` | Authenticate the CLI with your account via browser |
| `logout` | Remove stored credentials |
| `auth me` | Show the currently logged-in account |
| `auth security` | Show account security status |
| `config set-api <URL>` | Set the API endpoint |
| `config show` | Print the current config |
| `org create` | Create an organization |
| `org use <ORG>` | Set the default organization |
| `org list` | List your organizations |
| `repo create` | Create a repository |
| `repo list` | List repositories in the current org |
| `repo use <ORG/REPO>` | Set the active repository |
| `repo show` | Show the current repository |
| `repo update` | Update repository metadata |
| `repo archive / unarchive` | Archive or unarchive a repository |
| `repo delete / restore` | Delete or restore a repository |
| `init` | Initialize the current folder as a ClearMesh repo |
| `status` | Show staged and unstaged changes |
| `commit -m <MSG> [--key]` | Create a commit |
| `push [--key]` | Upload new chunks and record the commit |
| `clone <ORG/REPO> <PATH> [--key]` | Clone a repository to a local path |
| `sync [--key]` | Pull the latest commit and download new chunks |
| `log` | Show commit history |
| `branch list` | List branches |
| `branch create <NAME>` | Create a branch |
| `branch switch <NAME>` | Switch to a branch |
| `branch delete <NAME>` | Delete a branch |
| `merge <BRANCH> [--key]` | Merge a branch into the current branch |
| `conflicts` | List files with merge conflicts |
| `checkout <PATH> --ours\|--theirs` | Resolve a conflict by picking a side |
| `resolve <PATH>` | Mark a manually edited file as resolved |
| `mount <ORG/REPO> <PATH> [--key] [--branch]` | Mount a repository read-only via FUSE |
| `unmount <PATH>` | Unmount a mounted repository |
| `vfs` | Show VFS status |
| `vault status` | Show Vault connectivity and configuration |
| `doctor` | Run connectivity and configuration diagnostics |
| `version` | Print the CLI version |

---

## Troubleshooting

**`repo is encrypted; pass --key`**
The repository was created with encryption required. Add `--key "$PASSPHRASE"` to the command.

**`Transport endpoint is not connected`**
The FUSE mount process exited but the mount path was not cleaned up. Run `fusermount -u ./mnt` (Linux) or `umount ./mnt` (macOS).

**`permission denied` on mount**
FUSE is not available to your user. On Linux, your user may need to be in the `fuse` group: `sudo usermod -aG fuse $USER`, then log out and back in.

**Slow first read through a mounted path**
The first read triggers chunk downloads from the Vault. Subsequent reads of the same chunks are served from the local cache.

**`wrong API endpoint` or connection refused**
Check the configured endpoint with `clearmesh config show`. Reset it with `clearmesh config set-api https://api.clearmesh.net`. Run `clearmesh doctor` to verify.

**`not inside a ClearMesh repository`**
Run `clearmesh init` in the folder first, then `clearmesh repo use ORG/REPO` to link it to a repository.

**`nothing to push`**
Either the working directory has no changes, or the changes have not been committed yet. Run `clearmesh status` to check, then `clearmesh commit` before pushing.

**`passphrase length zero` or key errors**
The `--key` value is empty. Check that the environment variable is set: `echo "$PASSPHRASE"`.

**FUSE not installed**
Install `fuse3` via your package manager. On Debian/Ubuntu: `sudo apt install -y fuse3`. On Fedora: `sudo dnf install -y fuse3`.

---

## Security notes

- Encrypted repositories use client-side encryption. Your passphrase stays on your machine; only encrypted chunks are uploaded.
- Losing your passphrase means the encrypted files cannot be recovered. ClearMesh has no copy of it.
- `clearmesh login` uses device/browser authentication. Your account password is never handled by the CLI.
- Do not commit secrets or credentials to a repository unless you intentionally want them in its history.
- Public unencrypted repositories are readable by anyone. Do not put private data in them.
- The code has not gone through a formal external security audit yet.
- ClearMesh is beta. Keep backups of important data outside ClearMesh until it reaches a stable release.

---

## Current limitations

- Mount is read-only. Write-through mount is not currently supported.
- Windows is not supported. A Windows build and ProjFS-based mount backend are not yet available.
- There is no built-in diff or blame for binary files.
- No CI/CD integrations yet.
- The CLI does not support partial clone (selecting a subfolder of a repository).
- Large repositories with many files may be slow on initial clone due to sequential chunk downloads.

---

## Development

```bash
cargo fmt
cargo check
cargo test
cargo build --release
```

`target/` is in `.gitignore` and should not be committed.

To test against a local API instance, update the endpoint:

```bash
clearmesh config set-api http://localhost:7070
clearmesh doctor
```

---

## License

Apache-2.0. See [LICENSE](LICENSE).
