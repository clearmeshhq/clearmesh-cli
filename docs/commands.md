# ClearMesh CLI Commands

This guide explains every `clearmesh` command, what it does, when to use it, and examples for encrypted and unencrypted repositories.

## Basics

ClearMesh is a Git-like workflow for large files. Metadata lives in the ClearMesh API. File chunks live in Vault storage. Local repo state lives in `.clearmesh/` inside your working directory.

Common terms:

- `ORG`: organization slug, for example `acme`.
- `REPO`: repository slug, for example `dataset`.
- `ORG/REPO`: full repo target, for example `acme/dataset`.
- `--key`: passphrase used locally for encrypted repos. ClearMesh does not send this key to the API.

Encryption rules:

- Encrypted repos use `--encryption required` and require `--key` for `commit`, `sync`, `clone`, `merge`, and encrypted `mount`.
- Unencrypted repos use `--encryption none` and do not require `--key`.
- Public unencrypted repos can be cloned/read without a key if the API allows public access.
- Public encrypted repos expose metadata and file paths, but contents still require the key.

## First Setup

Point the CLI at an API:

```bash
clearmesh config set-api https://api.clearmesh.net
```

Create an account in the ClearMesh web console if needed, then approve the CLI with browser/device login:

```bash
clearmesh login
```

Create and select an org:

```bash
clearmesh org create --slug acme --name "Acme"
clearmesh org use acme
```

Create an encrypted repo:

```bash
clearmesh repo create --slug dataset --name "Dataset" --visibility private --encryption required
```

Create an unencrypted repo:

```bash
clearmesh repo create --slug artifacts --name "Artifacts" --visibility private --encryption none
```

Initialize a local folder:

```bash
mkdir dataset
cd dataset
clearmesh repo use acme/dataset
clearmesh init
```

## Auth Commands

### `clearmesh login`

Starts browser/device authentication and stores the approved session token locally.

```bash
clearmesh login
```

The CLI prints a verification URL and short user code. Open the URL in the web console, sign in there, approve the device, and leave the CLI polling until it completes. Signup, password login, password reset, and interactive 2FA challenges are web-only account flows.

### `clearmesh logout`

Revokes the current session when possible and removes the local token.

```bash
clearmesh logout
```

### `clearmesh auth me`

Shows the current authenticated user.

```bash
clearmesh auth me
```

### `clearmesh auth security`

Shows account security state, including 2FA status when supported by the API.

```bash
clearmesh auth security
```

2FA setup, recovery codes, and 2FA disable are handled in the web console settings.

## Config Commands

### `clearmesh config show`

Prints local CLI config.

```bash
clearmesh config show
```

This can include a token. Do not paste this output into public logs.

### `clearmesh config set-api`

Sets the API base URL.

```bash
clearmesh config set-api https://api.clearmesh.net
clearmesh config set-api http://127.0.0.1:7070
```

## Organization Commands

### `clearmesh org create`

Creates an organization and selects it as the current org.

```bash
clearmesh org create --slug acme --name "Acme"
```

Options:

- `--slug <SLUG>`: URL-safe org slug.
- `--name <NAME>`: display name.

### `clearmesh org use`

Selects the current org for future repo commands.

```bash
clearmesh org use acme
```

### `clearmesh org list`

Lists orgs the current user can access.

```bash
clearmesh org list
```

## Repository Commands

### `clearmesh repo create`

Creates a repository under the current org.

Encrypted private repo:

```bash
clearmesh repo create --slug dataset --name "Dataset" --visibility private --encryption required
```

Public unencrypted repo:

```bash
clearmesh repo create --slug public-artifacts --name "Public Artifacts" --visibility public --encryption none
```

Options:

- `--slug <SLUG>`: repo slug.
- `--name <NAME>`: display name.
- `--description <TEXT>`: optional description.
- `--visibility private|public`: default is `private`.
- `--encryption required|none`: default is `required`.

Notes:

- Encryption mode is a repo creation/default setting.
- Encryption can only change while the repo has zero commits.
- Visibility can be changed later by an owner/admin.

### `clearmesh repo update`

Updates repo settings for the selected repo.

```bash
clearmesh repo update --name "New Name"
clearmesh repo update --description "Training data for v2"
clearmesh repo update --visibility public
clearmesh repo update --encryption none
```

Options:

- `--name <NAME>`
- `--description <TEXT>`
- `--visibility private|public`
- `--encryption required|none`

`--encryption` only works before the first commit.

### `clearmesh repo list`

Lists repositories in the current org.

```bash
clearmesh repo list
```

### `clearmesh repo use`

Selects a repo globally and, if run inside a local ClearMesh repo, links that local repo to the selected remote.

```bash
clearmesh repo use dataset
clearmesh repo use acme/dataset
```

### `clearmesh repo show`

Shows selected repo details and a clone command.

```bash
clearmesh repo show
```

### `clearmesh repo archive`

Archives the selected repo. Archived repos should block writes.

```bash
clearmesh repo archive
```

### `clearmesh repo unarchive`

Unarchives the selected repo.

```bash
clearmesh repo unarchive
```

### `clearmesh repo delete`

Soft-deletes the selected repo. Vault objects and local files are retained.

```bash
clearmesh repo delete --confirm dataset
```

The `--confirm` value must exactly match the repo slug.

### `clearmesh repo restore`

Restores a soft-deleted repo when the API allows it.

```bash
clearmesh repo restore
```

## Local Repo Commands

### `clearmesh init`

Initializes `.clearmesh/` in the current directory.

```bash
mkdir dataset
cd dataset
clearmesh repo use acme/dataset
clearmesh init
```

This does not upload files. It only creates local metadata folders and links the folder to the selected org/repo when possible.

### `clearmesh status`

Shows local repo path, current branch, current HEAD, and tracked working files.

```bash
clearmesh status
```

### `clearmesh commit`

Creates a local ClearMesh commit from the working directory.

Encrypted repo:

```bash
echo "hello" > README.md
clearmesh commit --message "initial dataset" --key "$PASSPHRASE"
```

Unencrypted repo:

```bash
echo "hello" > README.md
clearmesh commit --message "initial artifacts"
```

Options:

- `-m, --message <MESSAGE>`: commit message.
- `--key <PASSPHRASE>`: required for encrypted repos.

This command writes objects and commit metadata locally. It does not upload to the API/Vault. Use `clearmesh push` after committing.

### `clearmesh push`

Uploads missing chunks and creates/updates the remote commit and branch.

Encrypted repo:

```bash
clearmesh push --key "$PASSPHRASE"
```

Unencrypted repo:

```bash
clearmesh push
```

Output includes:

```text
Chunks total: 33
Reused chunks: 32
Uploaded chunks: 1
Dedup saved: 32.0 MiB
```

The CLI asks the API which chunks already exist in the current repo and uploads only missing chunks.

### `clearmesh sync`

Fast-forwards the local branch from the remote branch.

Encrypted repo:

```bash
clearmesh sync --key "$PASSPHRASE"
```

Unencrypted repo:

```bash
clearmesh sync
```

If the local branch is ahead, it prints `Local branch ahead of remote`. If histories diverged, merge support is required.

### `clearmesh clone`

Creates a local folder, initializes ClearMesh metadata, links it to the repo, and syncs the default branch.

Encrypted repo:

```bash
clearmesh clone acme/dataset ./dataset-clone --key "$PASSPHRASE"
```

Unencrypted repo:

```bash
clearmesh clone acme/public-artifacts ./public-artifacts
```

Arguments:

- `<TARGET>`: `ORG/REPO`.
- `<PATH>`: destination directory.

### `clearmesh log`

Lists locally stored commits.

```bash
clearmesh log
```

Run `clearmesh sync` or `clearmesh clone` first if you need remote commits locally.

## Branch Commands

### `clearmesh branch list`

Lists remote branches for the linked repo.

```bash
clearmesh branch list
```

### `clearmesh branch create`

Creates a remote branch from the local current commit id.

```bash
clearmesh branch create experiment-1
```

Branch creation is metadata-only. It does not upload chunks or duplicate file bytes.

### `clearmesh branch switch`

Switches the local branch pointer and records the remote branch head locally.

```bash
clearmesh branch switch experiment-1
clearmesh sync --key "$PASSPHRASE"
```

Use `sync` after switching if you need the branch files materialized in the working tree.

### `clearmesh branch delete`

Deletes a remote branch when allowed by the API.

```bash
clearmesh branch delete experiment-1
```

## Merge and Conflict Commands

### `clearmesh merge`

Merges another local branch into the current local branch.

```bash
clearmesh merge experiment-1 --key "$PASSPHRASE"
```

For encrypted repos, `--key` is needed to materialize historical file contents. Clean merges update files and ask you to commit the merge.

### `clearmesh conflicts`

Lists merge conflicts.

```bash
clearmesh conflicts
```

Conflict files are written beside the original file:

- `file.OURS`
- `file.THEIRS`
- `file.BASE`

### `clearmesh checkout`

Chooses one side of a conflicted file.

```bash
clearmesh checkout --ours path/to/file.bin
clearmesh checkout --theirs path/to/file.bin
```

### `clearmesh resolve`

Marks a conflicted path resolved and removes side files.

```bash
clearmesh resolve path/to/file.bin
clearmesh commit --message "merge experiment-1" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
```

## Mount Commands

### `clearmesh mount`

Mounts a remote branch as a read-only filesystem. Files stream from Vault on demand.

Encrypted repo:

```bash
mkdir ./mnt
clearmesh mount acme/dataset ./mnt --key "$PASSPHRASE"
```

Unencrypted repo:

```bash
mkdir ./mnt
clearmesh mount acme/public-artifacts ./mnt
```

Specific branch:

```bash
clearmesh mount acme/dataset ./mnt --branch experiment-1 --key "$PASSPHRASE"
```

Options:

- `--key <PASSPHRASE>`: required for encrypted repos.
- `--branch <BRANCH>`: defaults to `main`.
- `--foreground`: keep the mount in the foreground when supported.

Linux requires FUSE3. macOS requires macFUSE for mount support. Windows mount is currently a stub and is not supported yet.

### `clearmesh unmount`

Unmounts a ClearMesh mount.

```bash
clearmesh unmount ./mnt
```

## Vault Commands

### `clearmesh vault status`

Shows Vault backend and storage status for the linked repo.

```bash
clearmesh vault status
```

Run inside a local ClearMesh repo linked to an org/repo.

## Utility Commands

### `clearmesh doctor`

Prints environment and feature support checks.

```bash
clearmesh doctor
```

### `clearmesh version`

Prints the CLI version.

```bash
clearmesh version
```

### `clearmesh --help`

Shows top-level command help.

```bash
clearmesh --help
clearmesh repo --help
clearmesh repo create --help
```

## Full Encrypted Workflow

```bash
clearmesh config set-api https://api.clearmesh.net
clearmesh login
clearmesh org use acme
clearmesh repo create --slug dataset --name "Dataset" --visibility private --encryption required

mkdir dataset
cd dataset
clearmesh repo use acme/dataset
clearmesh init

echo "hello ClearMesh" > README.md
clearmesh commit --message "initial dataset" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"

clearmesh clone acme/dataset ../dataset-clone --key "$PASSPHRASE"
```

## Full Unencrypted Workflow

```bash
clearmesh config set-api https://api.clearmesh.net
clearmesh login
clearmesh org use acme
clearmesh repo create --slug public-artifacts --name "Public Artifacts" --visibility public --encryption none

mkdir public-artifacts
cd public-artifacts
clearmesh repo use acme/public-artifacts
clearmesh init

echo "hello ClearMesh" > README.md
clearmesh commit --message "initial artifacts"
clearmesh push

clearmesh clone acme/public-artifacts ../public-artifacts-clone
```

## Troubleshooting

### Login required

Run:

```bash
clearmesh login
```

### Wrong API

Check and set the API URL:

```bash
clearmesh config show
clearmesh config set-api https://api.clearmesh.net
```

### Local repo has no org/repo

Link the folder:

```bash
clearmesh repo use acme/dataset
```

### Encrypted repo asks for a key

Use the same passphrase originally used for commits:

```bash
clearmesh sync --key "$PASSPHRASE"
clearmesh clone acme/dataset ./dataset --key "$PASSPHRASE"
```

ClearMesh cannot recover encrypted contents without the key.

### Working tree has uncommitted changes

Commit or remove local changes before syncing:

```bash
clearmesh status
clearmesh commit --message "save local changes" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
```

### Branch diverged

Use merge workflow:

```bash
clearmesh merge other-branch --key "$PASSPHRASE"
clearmesh conflicts
clearmesh checkout --ours path/to/file
clearmesh resolve path/to/file
clearmesh commit --message "merge other-branch" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
```

### Mount does not work

Check platform support:

```bash
clearmesh doctor
```

Linux needs FUSE3. macOS mount needs macFUSE. Windows mount is planned, but use `clearmesh clone` today.
