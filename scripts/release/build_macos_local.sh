#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="aarch64-apple-darwin"
INSTALL=0
PREFIX="${HOME}/.local/codex-vl"
BIN_DIR="${HOME}/.local/bin"
RELEASE_VERSION=""

usage() {
  cat <<'EOF'
Usage: scripts/release/build_macos_local.sh [--release-version VERSION] [--install] [--prefix DIR] [--bin-dir DIR]

Builds the macOS arm64 Codex VL npm payload from the current checkout, stages a
self-contained package tarball, and optionally installs it under ~/.local for
manual side-by-side testing.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release-version)
      RELEASE_VERSION="${2:-}"
      shift 2
      ;;
    --install)
      INSTALL=1
      shift
      ;;
    --prefix)
      PREFIX="${2:-}"
      shift 2
      ;;
    --bin-dir)
      BIN_DIR="${2:-}"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "${RELEASE_VERSION}" ]]; then
  RELEASE_VERSION="$(node -p "require('${REPO_ROOT}/codex-cli/package.json').version")"
fi

DIST_DIR="${REPO_ROOT}/dist/local-release"
VENDOR_ROOT="${DIST_DIR}/vendor-root"
STAGE_DIR="${DIST_DIR}/staging-codex"
TARBALL_PATH="${DIST_DIR}/codex-vl-${RELEASE_VERSION}-macos-local.tgz"

echo "==> Cleaning local release staging"
rm -rf "${VENDOR_ROOT}" "${STAGE_DIR}" "${TARBALL_PATH}"
mkdir -p "${DIST_DIR}"

echo "==> Ensuring Rust target ${TARGET}"
rustup target add "${TARGET}"

echo "==> Building native macOS arm64 codex binary"
cargo build \
  --manifest-path "${REPO_ROOT}/codex-rs/Cargo.toml" \
  --target "${TARGET}" \
  --release \
  --bin codex

echo "==> Preparing vendor payload"
python3 "${REPO_ROOT}/codex-cli/scripts/prepare_local_vendor.py" \
  --vendor-root "${VENDOR_ROOT}" \
  --target "${TARGET}" \
  --codex-binary "${REPO_ROOT}/codex-rs/target/${TARGET}/release/codex" \
  --include-rg

echo "==> Staging side-by-side npm package"
python3 "${REPO_ROOT}/codex-cli/scripts/build_npm_package.py" \
  --package codex \
  --release-version "${RELEASE_VERSION}" \
  --staging-dir "${STAGE_DIR}"

mkdir -p "${STAGE_DIR}/vendor"
cp -R "${VENDOR_ROOT}/${TARGET}" "${STAGE_DIR}/vendor/${TARGET}"
node -e "
const fs = require('fs');
const path = '${STAGE_DIR}/package.json';
const pkg = JSON.parse(fs.readFileSync(path, 'utf8'));
pkg.files = Array.from(new Set([...(pkg.files || []), 'vendor']));
fs.writeFileSync(path, JSON.stringify(pkg, null, 2) + '\n');
"

pack_json="$(
  cd "${STAGE_DIR}"
  npm pack --json --pack-destination "${DIST_DIR}"
)"
generated_tarball="$(node -e "const out = JSON.parse(process.argv[1]); console.log(out[0].filename);" "${pack_json}")"
mv "${DIST_DIR}/${generated_tarball}" "${TARBALL_PATH}"

echo "==> Local tarball ready at ${TARBALL_PATH}"

if [[ "${INSTALL}" -eq 1 ]]; then
  echo "==> Installing side-by-side into ${PREFIX}"
  rm -rf "${PREFIX}"
  mkdir -p "${PREFIX}" "${BIN_DIR}"
  npm install --prefix "${PREFIX}" "${TARBALL_PATH}"
  ln -sf "${PREFIX}/node_modules/.bin/codex-vl" "${BIN_DIR}/codex-vl"
  echo "==> Installed ${BIN_DIR}/codex-vl"
  echo "==> Shared runtime state remains under ~/.codex"
fi
