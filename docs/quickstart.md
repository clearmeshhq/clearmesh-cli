# Quickstart

```bash
clearmesh login
clearmesh org create --slug acme --name "Acme"
clearmesh org use acme
clearmesh repo create --slug data --name "Private Data"
clearmesh init
clearmesh repo use data
printf "private data\n" > data.txt
clearmesh commit --message "add data" --key "$PASSPHRASE"
clearmesh push --key "$PASSPHRASE"
```

Clone and sync:

```bash
clearmesh clone acme/data ./data-copy --key "$PASSPHRASE"
```

## Mount repositories on demand

Read-only Linux Mount in Private Beta lets tools see files immediately from metadata. Files stream from Vault only when your tools read them.

```bash
sudo apt install -y fuse3
clearmesh mount stacknow/testrepo ./testrepo
clearmesh mount stacknow/encrypted-repo ./encrypted-repo --key demo-passphrase
ls ./testrepo
cat ./testrepo/README.md
clearmesh unmount ./testrepo
```

Mount is read-only and Linux/FUSE3 only. ClearMesh fetches metadata upfront, then fetches chunks from Vault on read. Encrypted repos require `--key`; unencrypted repos mount without a key. Encrypted chunks are cached locally at `~/.cache/clearmesh/objects`; plaintext chunks are never written to disk and decrypted data stays in memory only. No full sync is required.

## Branches and merge

Branch commands:

```bash
clearmesh branch list
clearmesh branch create feature-x
clearmesh branch switch feature-x
clearmesh branch delete feature-x
clearmesh merge BRANCH
clearmesh merge BRANCH --key KEY
clearmesh conflicts
clearmesh checkout --ours PATH
clearmesh checkout --theirs PATH
clearmesh resolve PATH
```

Merge is file-level and binary-safe. ClearMesh does not attempt text line merges. Fast-forward merge is supported. If different files changed, ClearMesh can apply both sides and let you create a merge commit. If the same path changed on both sides, ClearMesh writes conflict side files:

```text
PATH.OURS
PATH.THEIRS
PATH.BASE
```

Resolve manually, then mark the path resolved and commit:

```bash
clearmesh resolve PATH
clearmesh commit --message "resolve merge" --key demo-passphrase
```

Resolved merge commits have two parents. Run `clearmesh sync` before merging remote branches. Merge currently uses local commit metadata and does not fetch missing remote commits automatically.

Example workflow:

```bash
clearmesh branch create experiment
clearmesh branch switch experiment
echo "experiment" > model-notes.txt
clearmesh commit --message "experiment notes" --key demo-passphrase

clearmesh branch switch main
clearmesh merge experiment --key demo-passphrase
clearmesh commit --message "merge experiment" --key demo-passphrase
clearmesh push --key demo-passphrase
```

Conflict example:

```bash
clearmesh merge feature-x --key demo-passphrase
clearmesh conflicts
clearmesh checkout --theirs data/model.bin
clearmesh resolve data/model.bin
clearmesh commit --message "resolve model conflict" --key demo-passphrase
```
