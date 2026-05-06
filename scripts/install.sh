#!/usr/bin/env bash
set -euo pipefail

OS="$(uname -s)"
ARCH="$(uname -m)"

BASE_URL="${CLEARMESH_BASE_URL:-https://clearmesh.net/releases/latest}"
INSTALL_DIR="${CLEARMESH_INSTALL_DIR:-$HOME/.local/bin}"
SOURCE_REPO="${CLEARMESH_SOURCE_REPO:-https://github.com/clearmeshhq/clearmesh-cli.git}"
CORE_REPO="${CLEARMESH_CORE_REPO:-https://github.com/clearmeshhq/clearmesh-core.git}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$INSTALL_DIR"

has_command() {
  command -v "$1" >/dev/null 2>&1
}

install_from_archive() {
  local name="$1"
  local archive="$name.tar.gz"

  echo "Downloading ClearMesh CLI..."
  curl -fsSL "$BASE_URL/$archive" -o "$TMP/$archive"
  curl -fsSL "$BASE_URL/$archive.sha256" -o "$TMP/$archive.sha256"

  echo "Verifying checksum..."
  (
    cd "$TMP"
    sha256sum -c "$archive.sha256"
  )

  echo "Installing..."
  tar -xzf "$TMP/$archive" -C "$TMP"

  local bin
  bin="$(find "$TMP" -type f -name clearmesh -perm -111 | head -n 1)"
  if [ -z "$bin" ]; then
    bin="$(find "$TMP" -type f -name clearmesh | head -n 1)"
  fi
  if [ -z "$bin" ]; then
    echo "error: clearmesh binary not found in archive." >&2
    echo "archive contents:" >&2
    tar -tzf "$TMP/$archive" >&2
    exit 1
  fi

  install -m 0755 "$bin" "$INSTALL_DIR/clearmesh"

  echo ""
  echo "Installed clearmesh -> $INSTALL_DIR/clearmesh"
}

install_from_source() {
  echo "No prebuilt ClearMesh CLI artifact is available for this platform."
  echo "Building from source instead..."

  if ! has_command git; then
    echo "error: git is required to build from source." >&2
    exit 1
  fi

  if ! has_command cargo; then
    echo "error: Rust/Cargo is required to build from source." >&2
    echo ""
    echo "Install Rust first:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "  source \"\$HOME/.cargo/env\""
    echo ""
    echo "Then rerun:"
    echo "  curl -fsSL $BASE_URL/install.sh | bash"
    exit 1
  fi

  if [ "$OS" = "Darwin" ]; then
    if ! xcode-select -p >/dev/null 2>&1; then
      echo "error: Xcode Command Line Tools are required on macOS." >&2
      echo "Install them with:"
      echo "  xcode-select --install"
      exit 1
    fi
  fi

  git clone --depth 1 "$SOURCE_REPO" "$TMP/clearmesh-cli"
  git clone --depth 1 "$CORE_REPO" "$TMP/clearmesh-core"
  cd "$TMP/clearmesh-cli"

  cargo build --release -p clearmesh-cli

  if [ -f "target/release/clearmesh" ]; then
    BIN="target/release/clearmesh"
  elif [ -f "target/release/clearmesh-cli" ]; then
    BIN="target/release/clearmesh-cli"
  else
    echo "error: built binary not found." >&2
    exit 1
  fi

  install -m 0755 "$BIN" "$INSTALL_DIR/clearmesh"

  echo ""
  echo "Built and installed clearmesh -> $INSTALL_DIR/clearmesh"
  if [ "$OS" = "Darwin" ]; then
    echo "Normal CLI commands work from this source build. Read-only mount may require macFUSE."
  fi
}

case "$OS:$ARCH" in
  Linux:x86_64)
    NAME="clearmesh-linux-x86_64"
    install_from_archive "$NAME"
    ;;

  Darwin:x86_64)
    NAME="clearmesh-macos-x86_64"
    if curl -fsI "$BASE_URL/$NAME.tar.gz" >/dev/null 2>&1; then
      install_from_archive "$NAME"
    else
      install_from_source
    fi
    ;;

  Darwin:arm64)
    NAME="clearmesh-macos-aarch64"
    if curl -fsI "$BASE_URL/$NAME.tar.gz" >/dev/null 2>&1; then
      install_from_archive "$NAME"
    else
      install_from_source
    fi
    ;;

  *)
    echo "error: unsupported platform: OS=$OS ARCH=$ARCH" >&2
    echo ""
    echo "Supported by this installer:"
    echo "  Linux x86_64"
    echo "  macOS x86_64"
    echo "  macOS arm64"
    echo ""
    echo "Windows PowerShell:"
    echo "  irm $BASE_URL/install.ps1 | iex"
    exit 1
    ;;
esac

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "Add this to your shell profile:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

echo ""
echo "Next:"
echo "  clearmesh config set-api https://api.clearmesh.net"
echo "  clearmesh --help"
