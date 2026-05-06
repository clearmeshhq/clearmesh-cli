#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
PUBLISH_DIR="${CLEARMESH_RELEASE_PUBLISH_DIR:-/var/www/clearmesh/releases/latest}"
VERSION="$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/crates/clearmesh-cli/Cargo.toml")"

mkdir -p "$DIST"

log() {
  echo ""
  echo "==> $*"
}

warn() {
  echo "warning: $*" >&2
}

sha_file() {
  local file="$1"
  (cd "$(dirname "$file")" && sha256sum "$(basename "$file")" > "$(basename "$file").sha256")
}

publish_file() {
  local file="$1"
  local sha="$file.sha256"

  if [ -d "$PUBLISH_DIR" ]; then
    cp "$file" "$sha" "$PUBLISH_DIR/"
    echo "Published $(basename "$file")"
  else
    warn "publish dir missing: $PUBLISH_DIR"
  fi
}

find_unix_bin() {
  local dir="$1"
  if [ -f "$dir/clearmesh" ]; then echo "$dir/clearmesh"; return 0; fi
  if [ -f "$dir/clearmesh-cli" ]; then echo "$dir/clearmesh-cli"; return 0; fi
  return 1
}

find_windows_bin() {
  local dir="$1"
  if [ -f "$dir/clearmesh.exe" ]; then echo "$dir/clearmesh.exe"; return 0; fi
  if [ -f "$dir/clearmesh-cli.exe" ]; then echo "$dir/clearmesh-cli.exe"; return 0; fi
  return 1
}

package_tar() {
  local name="$1"
  local bin="$2"
  local staging

  staging="$(mktemp -d)"

  mkdir -p "$staging/$name"
  cp "$bin" "$staging/$name/clearmesh"
  chmod +x "$staging/$name/clearmesh"

  cat > "$staging/$name/README.txt" <<TXT
ClearMesh CLI

Install:
  mkdir -p "\$HOME/.local/bin"
  cp clearmesh "\$HOME/.local/bin/clearmesh"
  chmod +x "\$HOME/.local/bin/clearmesh"

Quick start:
  clearmesh config set-api https://api.clearmesh.net
TXT

  tar -C "$staging" -czf "$DIST/$name.tar.gz" "$name"
  rm -rf "$staging"

  sha_file "$DIST/$name.tar.gz"
  publish_file "$DIST/$name.tar.gz"

  echo "Built $DIST/$name.tar.gz"
}

package_zip() {
  local name="$1"
  local bin="$2"
  local staging

  command -v zip >/dev/null || {
    warn "zip missing; install zip first"
    return 1
  }

  staging="$(mktemp -d)"

  mkdir -p "$staging/$name"
  cp "$bin" "$staging/$name/clearmesh.exe"

  cat > "$staging/$name/README.txt" <<'TXT'
ClearMesh CLI for Windows

Install:
1. Copy clearmesh.exe somewhere permanent.
2. Add that folder to PATH.

Quick start:
  clearmesh config set-api https://api.clearmesh.net
TXT

  (cd "$staging" && zip -qr "$DIST/$name.zip" "$name")
  rm -rf "$staging"

  sha_file "$DIST/$name.zip"
  publish_file "$DIST/$name.zip"

  echo "Built $DIST/$name.zip"
}

build_linux_x86_64() {
  log "Building Linux x86_64"

  cargo build --release -p clearmesh-cli --target x86_64-unknown-linux-gnu

  local bin
  bin="$(find_unix_bin "$ROOT/target/x86_64-unknown-linux-gnu/release")"

  package_tar "clearmesh-linux-x86_64-${VERSION}" "$bin"
}

build_windows_x86_64() {
  log "Building Windows x86_64"

  if ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
    warn "mingw-w64 missing; run ./scripts/bootstrap-release-toolchains.sh"
    return 0
  fi

  rustup target add x86_64-pc-windows-gnu >/dev/null

  cargo build --release -p clearmesh-cli --target x86_64-pc-windows-gnu

  local bin
  bin="$(find_windows_bin "$ROOT/target/x86_64-pc-windows-gnu/release")"

  package_zip "clearmesh-windows-x86_64-${VERSION}" "$bin"
}

build_macos_from_macos() {
  log "Building macOS on macOS"

  local arch
  arch="$(uname -m)"

  cargo build --release -p clearmesh-cli

  local bin
  bin="$(find_unix_bin "$ROOT/target/release")"

  if [ "$arch" = "arm64" ]; then
    package_tar "clearmesh-macos-aarch64-${VERSION}" "$bin"
  elif [ "$arch" = "x86_64" ]; then
    package_tar "clearmesh-macos-x86_64-${VERSION}" "$bin"
  else
    warn "unsupported macOS arch: $arch"
  fi
}

build_macos_from_linux_osxcross() {
  log "Building macOS from Linux via osxcross"

  local built=0

  if command -v o64-clang >/dev/null; then
    log "Building macOS x86_64"

    rustup target add x86_64-apple-darwin >/dev/null

    CC_x86_64_apple_darwin=o64-clang \
    CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=o64-clang \
      cargo build --release -p clearmesh-cli --target x86_64-apple-darwin

    local bin
    bin="$(find_unix_bin "$ROOT/target/x86_64-apple-darwin/release")"

    package_tar "clearmesh-macos-x86_64-${VERSION}" "$bin"
    built=1
  fi

  if command -v oa64-clang >/dev/null; then
    log "Building macOS aarch64"

    rustup target add aarch64-apple-darwin >/dev/null

    CC_aarch64_apple_darwin=oa64-clang \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=oa64-clang \
      cargo build --release -p clearmesh-cli --target aarch64-apple-darwin

    local bin
    bin="$(find_unix_bin "$ROOT/target/aarch64-apple-darwin/release")"

    package_tar "clearmesh-macos-aarch64-${VERSION}" "$bin"
    built=1
  fi

  if [ "$built" = "0" ]; then
    warn "osxcross not found; skipping macOS builds"
    warn "run this script on macOS, or configure osxcross with Apple SDK"
  fi
}

write_latest_aliases() {
  log "Writing latest aliases"

  local linux="$DIST/clearmesh-linux-x86_64-${VERSION}.tar.gz"
  local windows="$DIST/clearmesh-windows-x86_64-${VERSION}.zip"
  local macos_x64="$DIST/clearmesh-macos-x86_64-${VERSION}.tar.gz"
  local macos_arm="$DIST/clearmesh-macos-aarch64-${VERSION}.tar.gz"

  if [ -f "$linux" ]; then
    cp "$linux" "$DIST/clearmesh-linux-x86_64.tar.gz"
    sha_file "$DIST/clearmesh-linux-x86_64.tar.gz"
    publish_file "$DIST/clearmesh-linux-x86_64.tar.gz"
  else
    warn "Linux artifact missing; no latest Linux alias written"
  fi

  if [ -f "$windows" ]; then
    cp "$windows" "$DIST/clearmesh-windows-x86_64.zip"
    sha_file "$DIST/clearmesh-windows-x86_64.zip"
    publish_file "$DIST/clearmesh-windows-x86_64.zip"
  else
    warn "Windows artifact missing; no latest Windows alias written"
  fi

  if [ -f "$macos_x64" ]; then
    cp "$macos_x64" "$DIST/clearmesh-macos-x86_64.tar.gz"
    sha_file "$DIST/clearmesh-macos-x86_64.tar.gz"
    publish_file "$DIST/clearmesh-macos-x86_64.tar.gz"
  else
    warn "macOS x86_64 artifact missing; no latest macOS x86_64 alias written"
  fi

  if [ -f "$macos_arm" ]; then
    cp "$macos_arm" "$DIST/clearmesh-macos-aarch64.tar.gz"
    sha_file "$DIST/clearmesh-macos-aarch64.tar.gz"
    publish_file "$DIST/clearmesh-macos-aarch64.tar.gz"
  else
    warn "macOS aarch64 artifact missing; no latest macOS aarch64 alias written"
  fi
}

write_manifest() {
  log "Writing manifest"

  python3 - <<PY
import json
from pathlib import Path

dist = Path("$DIST")
items = []

for p in sorted(dist.iterdir()):
    if not p.is_file():
        continue
    if not p.name.startswith("clearmesh-"):
        continue
    if p.name.endswith(".sha256"):
        continue

    sha = dist / (p.name + ".sha256")
    items.append({
        "name": p.name,
        "bytes": p.stat().st_size,
        "sha256": sha.read_text().split()[0] if sha.exists() else "",
    })

manifest = {
    "product": "clearmesh-cli",
    "version": "$VERSION",
    "channel": "latest",
    "artifacts": items,
}

out = dist / "manifest.json"
out.write_text(json.dumps(manifest, indent=2) + "\\n")
print(out.read_text())
PY

  if [ -d "$PUBLISH_DIR" ]; then
    cp "$DIST/manifest.json" "$PUBLISH_DIR/"
  fi
}

main() {
  cd "$ROOT"

  log "ClearMesh CLI release build"
  echo "Version: $VERSION"
  echo "Host OS: $(uname -s)"
  echo "Dist: $DIST"
  echo "Publish: $PUBLISH_DIR"

  case "$(uname -s)" in
    Linux)
      build_linux_x86_64
      build_windows_x86_64
      build_macos_from_linux_osxcross
      ;;
    Darwin)
      build_macos_from_macos
      ;;
    *)
      echo "error: unsupported host OS: $(uname -s)" >&2
      exit 1
      ;;
  esac

  write_latest_aliases
  write_manifest

  log "Artifacts"
  ls -lh "$DIST"/clearmesh-* 2>/dev/null || true
}

main "$@"
