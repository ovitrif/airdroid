#!/bin/sh
set -eu

REPO="${AIRADB_INSTALL_REPO:-ovitrif/airadb}"
BINARY="airadb"
GITHUB_URL="${AIRADB_INSTALL_GITHUB_URL:-https://github.com}"

main() {
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os" in
    linux)  os="unknown-linux-musl" ;;
    darwin) os="apple-darwin" ;;
    *) echo "Unsupported OS: $os" >&2; exit 1 ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
  esac

  target="${arch}-${os}"
  latest_url="${GITHUB_URL}/${REPO}/releases/latest"

  install_dir="$(resolve_install_dir)"

  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT

  tag="$(resolve_tag "$latest_url")"
  archive="${BINARY}-${tag}-${target}.tar.gz"
  base_url="${GITHUB_URL}/${REPO}/releases/download/${tag}"

  echo "Installing ${BINARY} ${tag} for ${target}..."

  download_release "$tag" "$archive" "$base_url" "$tmpdir"

  cd "$tmpdir"
  expected_checksum="$(awk -v file="$archive" '$2 == file { print $1; exit }' checksums.sha256)"
  if [ -z "$expected_checksum" ]; then
    echo "Missing checksum entry for ${archive}" >&2
    exit 1
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    actual_checksum="$(sha256sum "$archive" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual_checksum="$(shasum -a 256 "$archive" | awk '{print $1}')"
  else
    echo "Warning: cannot verify checksum (no sha256sum or shasum found)" >&2
    actual_checksum=""
  fi

  if [ -n "$actual_checksum" ] && [ "$actual_checksum" != "$expected_checksum" ]; then
    echo "Checksum verification failed for ${archive}" >&2
    exit 1
  fi

  tar xzf "$archive"
  dir="${BINARY}-${tag}-${target}"
  install -m 755 "${dir}/${BINARY}" "${install_dir}/${BINARY}"

  echo "Installed to ${install_dir}/${BINARY}"
  if "${install_dir}/${BINARY}" install-shell --bin-dir "$install_dir"; then
    echo "Installed 'aw' alias (android wifi) and zsh completions when available."
    echo "Run 'aw' to get started."
  else
    echo "Shell integration skipped. Run '${BINARY} install-shell --force' to retry." >&2
    echo "Run '${BINARY}' to get started."
  fi
}

resolve_install_dir() {
  if [ -n "${AIRADB_INSTALL_DIR:-}" ]; then
    mkdir -p "$AIRADB_INSTALL_DIR" 2>/dev/null || true
    if [ -w "$AIRADB_INSTALL_DIR" ]; then
      printf '%s\n' "$AIRADB_INSTALL_DIR"
      return
    fi

    echo "AIRADB_INSTALL_DIR is not writable: ${AIRADB_INSTALL_DIR}" >&2
    exit 1
  fi

  if [ -w /usr/local/bin ]; then
    printf '%s\n' "/usr/local/bin"
  elif [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
    printf '%s\n' "$HOME/.local/bin"
  else
    echo "Cannot find writable install directory." >&2
    echo "Run with sudo, set AIRADB_INSTALL_DIR, or create ~/.local/bin" >&2
    exit 1
  fi
}

resolve_tag() {
  latest_url="$1"

  if [ -n "${AIRADB_INSTALL_TAG:-}" ]; then
    printf '%s\n' "$AIRADB_INSTALL_TAG"
    return
  fi

  if command -v gh >/dev/null 2>&1; then
    tag="$(gh release view --repo "$REPO" --json tagName -q '.tagName' 2>/dev/null || true)"
    if [ -n "$tag" ]; then
      printf '%s\n' "$tag"
      return
    fi
  fi

  url="$(curl -fsSL -o /dev/null -w '%{url_effective}' "$latest_url" || true)"
  tag="${url##*/}"
  if [ -n "$tag" ] && [ "$tag" != "latest" ]; then
    printf '%s\n' "$tag"
    return
  fi

  echo "Failed to resolve the latest release tag for ${REPO}." >&2
  exit 1
}

download_release() {
  tag="$1"
  archive="$2"
  base_url="$3"
  tmpdir="$4"

  if command -v gh >/dev/null 2>&1; then
    if gh release download "$tag" \
      --repo "$REPO" \
      --pattern "$archive" \
      --pattern checksums.sha256 \
      --dir "$tmpdir" \
      --clobber >/dev/null 2>&1; then
      return
    fi
  fi

  if curl -fsSL "${base_url}/${archive}" -o "${tmpdir}/${archive}" &&
    curl -fsSL "${base_url}/checksums.sha256" -o "${tmpdir}/checksums.sha256"; then
    return
  fi

  echo "Failed to download release assets for ${tag}." >&2
  exit 1
}

main
