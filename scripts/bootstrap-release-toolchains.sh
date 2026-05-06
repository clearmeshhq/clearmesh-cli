#!/usr/bin/env bash
set -euo pipefail

echo "==> Installing release build dependencies"

OS="$(uname -s)"

if [ "$OS" = "Linux" ]; then
  if command -v apt >/dev/null; then
    sudo apt update
    sudo apt install -y build-essential pkg-config curl ca-certificates zip unzip tar mingw-w64
  else
    echo "warning: non-apt Linux detected. Install manually: build-essential/pkg-config/zip/mingw-w64" >&2
  fi

  rustup target add x86_64-unknown-linux-gnu
  rustup target add x86_64-pc-windows-gnu

  echo ""
  echo "Linux and Windows targets are ready."

  if command -v o64-clang >/dev/null && command -v oa64-clang >/dev/null; then
    rustup target add x86_64-apple-darwin
    rustup target add aarch64-apple-darwin
    echo "osxcross detected. macOS cross-build targets are ready."
  else
    echo "macOS cross-build not configured."
    echo "To build macOS from Linux, install osxcross with an Apple SDK so o64-clang and oa64-clang exist."
  fi
elif [ "$OS" = "Darwin" ]; then
  rustup target add x86_64-apple-darwin
  rustup target add aarch64-apple-darwin
  echo "macOS targets are ready."
else
  echo "unsupported host OS: $OS" >&2
  exit 1
fi

echo ""
echo "Done."
