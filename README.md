# ClearMesh CLI

ClearMesh is a Git-like CLI for large file repositories. It chunks files locally, creates commits, uploads chunks through signed Vault URLs, and can clone, sync, and mount repositories.

## Install

Linux:

```bash
curl -fsSL https://clearmesh.net/releases/latest/install.sh | bash
```

Windows:

Download the Windows zip from ClearMesh releases when available, extract `clearmesh.exe`, and add it to `PATH`.

macOS:

Native packages are coming soon. For now, build from source:

```bash
cargo build --release -p clearmesh-cli
```

## Configure

```bash
clearmesh config set-api https://api.clearmesh.net
clearmesh doctor
```

## Log In

```bash
clearmesh login
```

The CLI uses browser/device authentication. Account signup, password login, password reset, and 2FA prompts stay in the ClearMesh web console.

## Organizations And Repositories

```bash
clearmesh org create --slug acme --name "Acme"
clearmesh org use acme

clearmesh repo create --slug data --name "Private Data" --visibility private --encryption required
clearmesh repo create --slug open-data --name "Open Data" --visibility public --encryption none
```

Private repositories require organization membership. Public repositories can be read without membership. Encrypted repositories require a key for file contents. Public unencrypted repositories do not require a key.

## Commit And Push

Encrypted repository:

```bash
mkdir data && cd data
clearmesh repo use acme/data
clearmesh init
printf "private data\n" > data.txt
clearmesh commit --message "add data" --key "$PASSPHRASE"
clearmesh push
```

Unencrypted repository:

```bash
clearmesh repo use acme/open-data
clearmesh commit --message "add public data"
clearmesh push
```

## Clone And Sync

Encrypted:

```bash
clearmesh clone acme/data ./data-copy --key "$PASSPHRASE"
clearmesh sync --key "$PASSPHRASE"
```

Public unencrypted:

```bash
clearmesh clone acme/open-data ./open-data-copy
clearmesh sync
```

## Mount

Linux and macOS use the FUSE/macFUSE backend. Linux requires FUSE3.

```bash
sudo apt install -y fuse3
clearmesh mount acme/data ./mnt
clearmesh mount acme/private-data ./mnt --key "$PASSPHRASE"
clearmesh unmount ./mnt
```

Windows mount is not supported yet. A ProjFS backend is planned.

## Security Model

Encrypted repositories use client-side encryption. The CLI derives a local key from your passphrase and uploads encrypted chunks. ClearMesh services should never receive your passphrase or derived key.

Unencrypted repositories store readable chunks. Public unencrypted repositories are designed to be cloned and read without a key.

Account credentials are not collected by the CLI. Use `clearmesh login` to approve the CLI from the web console.
