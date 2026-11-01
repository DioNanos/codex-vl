#!/bin/sh

set -eu

# codex-vl standalone installer.
#
# Installs the `codex-vl` command from the DioNanos/codex-vl GitHub releases
# into ${CODEX_HOME:-~/.codex}/packages/standalone-vl/ and symlinks
# ~/.local/bin/codex-vl (or $PREFIX/bin on Termux) at it. Verification chain:
# GitHub asset digest of SHA256SUMS -> SHA256SUMS -> archive sha256 -> staging
# extraction -> `bin/codex --version` -> atomic move + `current` symlink.
#
# This installer never touches the official `codex` command,
# ~/.codex/packages/standalone, Homebrew, or the official Codex npm package,
# and it never runs as root. Automatic updates are disabled by design:
# re-run this installer (optionally with CODEX_RELEASE=<version>) to update.

RELEASE="${CODEX_RELEASE:-latest}"
NON_INTERACTIVE="${CODEX_NON_INTERACTIVE:-false}"
BIN_DIR="${CODEX_INSTALL_DIR:-}"
MODIFY_PATH="1"
UNINSTALL="0"
ASSETS_DIR="${CODEX_INSTALL_ASSETS_DIR:-}"

# --- BEGIN embedded release checksums (release packaging fills this block) ---
# Release-pinned installer variants attached to a GitHub release ship this
# block filled in; the generic installer leaves it empty and resolves
# everything through the GitHub API. When the block is filled, the installer
# verifies the archive directly against the embedded digest and skips the
# GitHub API manifest fetch entirely (no anonymous rate limits).
EMBEDDED_RELEASE_VERSION=""
embedded_package_digest() {
  return 1
}
# --- END embedded release checksums ---

GITHUB_REPO="DioNanos/codex-vl"
NPM_PACKAGE="@mmmbuto/codex-vl"
COMMAND_NAME="codex-vl"
CODEX_HOME_DIR="${CODEX_HOME:-$HOME/.codex}"
STANDALONE_ROOT="$CODEX_HOME_DIR/packages/standalone-vl"
RELEASES_DIR="$STANDALONE_ROOT/releases"
CURRENT_LINK="$STANDALONE_ROOT/current"
LOCK_FILE="$STANDALONE_ROOT/install.lock"
LOCK_DIR="$STANDALONE_ROOT/install.lock.d"
LOCK_STALE_AFTER_SECS=600
CHECKSUM_ASSET="SHA256SUMS"
PACKAGE_PREFIX="codex-vl-package-"

path_action="already"
path_profile=""
conflict_path=""
lock_kind=""
tmp_dir=""
os=""
arch=""
vendor_target=""
platform_label=""
resolved_version=""

step() {
  printf '==> %s\n' "$1"
}

warn() {
  printf 'WARNING: %s\n' "$1" >&2
}

die() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

usage() {
  cat <<EOF
Usage: install.sh [--release VERSION] [--uninstall] [--no-modify-path]
                  [--bin-dir DIR] [--from-dir DIR]

Installs the codex-vl CLI from the DioNanos/codex-vl GitHub releases.

The install is self-contained: it only writes
\${CODEX_HOME:-~/.codex}/packages/standalone-vl/ and a \`codex-vl\` symlink.
It never touches the official \`codex\` command, ~/.codex/packages/standalone,
Homebrew, or any npm install. No sudo; automatic updates are
disabled by design.

Options:
  --release VERSION   Install a specific release (default: latest).
  --uninstall         Remove the standalone-vl tree and the codex-vl symlink.
  --no-modify-path    Do not edit any shell profile file.
  --bin-dir DIR       Directory for the codex-vl symlink
                      (default: ~/.local/bin; \$PREFIX/bin on Termux).
  --from-dir DIR      Verify and install from local release assets instead of
                      downloading (testing aid; requires --release).

Environment:
  CODEX_RELEASE           Version to install; overridden by --release.
  CODEX_NON_INTERACTIVE   Set to 1, true, or yes to skip prompts.
  CODEX_INSTALL_DIR       Same as --bin-dir.
  CODEX_INSTALL_ASSETS_DIR  Same as --from-dir.
  CODEX_HOME              Codex home (default: ~/.codex).
EOF
}

normalize_version() {
  case "$1" in
    "" | latest)
      printf 'latest\n'
      ;;
    rust-v*)
      printf '%s\n' "${1#rust-v}"
      ;;
    v*)
      printf '%s\n' "${1#v}"
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}

validate_version() {
  version="$1"

  if [ "$version" = "latest" ]; then
    return
  fi

  if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.]*)?$'; then
    die "invalid codex-vl release version: $version. Expected latest or x.y.z[-prerelease]."
  fi
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --release)
        if [ "$#" -lt 2 ]; then
          die "--release requires a value."
        fi
        RELEASE="$2"
        shift
        ;;
      --bin-dir)
        if [ "$#" -lt 2 ]; then
          die "--bin-dir requires a value."
        fi
        BIN_DIR="$2"
        shift
        ;;
      --from-dir)
        if [ "$#" -lt 2 ]; then
          die "--from-dir requires a value."
        fi
        ASSETS_DIR="$2"
        shift
        ;;
      --uninstall)
        UNINSTALL="1"
        ;;
      --no-modify-path)
        MODIFY_PATH="0"
        ;;
      --help | -h)
        usage
        exit 0
        ;;
      *)
        die "unknown argument: $1"
        ;;
    esac
    shift
  done
}

download_file() {
  url="$1"
  output="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$output"
    return
  fi

  if command -v wget >/dev/null 2>&1; then
    wget -q -O "$output" "$url"
    return
  fi

  die "curl or wget is required to install codex-vl."
}

download_text() {
  url="$1"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url"
    return
  fi

  if command -v wget >/dev/null 2>&1; then
    wget -q -O - "$url"
    return
  fi

  die "curl or wget is required to install codex-vl."
}

parse_release_metadata() {
  # Bound awk's record size so compact, single-line JSON stays fast on every
  # supported awk implementation. JSON strings cannot contain literal newlines,
  # so the record boundaries inserted by fold do not change the document.
  LC_ALL=C fold -b -w 4096 | LC_ALL=C awk '
    function finish_string(value) {
      if (object_depth == 1 &&
          (key == "tag_name" || key == "version" || key == "target" || key == "entrypoint")) {
        print key "\t" value
      } else if (object_depth == asset_object_depth) {
        if (key == "name") {
          asset_name = value
        } else if (key == "digest") {
          asset_digest = value
        }
      }

      expecting_value = 0
      key = ""
    }

    {
      for (i = 1; i <= length($0); i++) {
        char = substr($0, i, 1)

        if (in_string) {
          if (escaped) {
            token = token "\\" char
            escaped = 0
          } else if (char == "\\") {
            escaped = 1
          } else if (char == "\"") {
            in_string = 0
            if (string_is_value) {
              finish_string(token)
            } else {
              pending_key = token
            }
          } else {
            token = token char
          }
          continue
        }

        if (char == "\"") {
          in_string = 1
          token = ""
          escaped = 0
          string_is_value = expecting_value
        } else if (char == ":" && pending_key != "") {
          key = pending_key
          pending_key = ""
          expecting_value = 1
        } else if (char == "{") {
          object_depth++
          if (assets_array_depth != 0 &&
              array_depth == assets_array_depth &&
              asset_object_depth == 0) {
            asset_object_depth = object_depth
            asset_name = ""
            asset_digest = ""
          }
          expecting_value = 0
          key = ""
        } else if (char == "}") {
          if (object_depth == asset_object_depth) {
            if (asset_name != "" && asset_digest != "") {
              print "asset\t" asset_name "\t" asset_digest
            }
            asset_object_depth = 0
            asset_name = ""
            asset_digest = ""
          }
          object_depth--
          expecting_value = 0
          key = ""
          pending_key = ""
        } else if (char == "[") {
          array_depth++
          if (expecting_value && key == "assets" && object_depth == 1) {
            assets_array_depth = array_depth
          }
          expecting_value = 0
          key = ""
        } else if (char == "]") {
          if (array_depth == assets_array_depth) {
            assets_array_depth = 0
          }
          array_depth--
          expecting_value = 0
          key = ""
          pending_key = ""
        } else if (char == ",") {
          expecting_value = 0
          key = ""
          pending_key = ""
        }
      }
    }

    END {
      if (in_string || object_depth != 0 || array_depth != 0) {
        exit 1
      }
    }
  '
}

release_url_for_asset() {
  asset="$1"
  resolved_version="$2"

  printf 'https://github.com/%s/releases/download/v%s/%s\n' "$GITHUB_REPO" "$resolved_version" "$asset"
}

release_metadata_url() {
  resolved_version="$1"

  printf 'https://api.github.com/repos/%s/releases/tags/v%s\n' "$GITHUB_REPO" "$resolved_version"
}

release_asset_digest_or_empty() {
  asset="$1"

  digest="$(printf '%s\n' "$release_metadata" | awk -F '\t' -v asset="$asset" '
    $1 == "asset" && $2 == asset {
      print $3
      exit
    }
  ')"

  case "$digest" in
    sha256:????????????????????????????????????????????????????????????????)
      printf '%s\n' "${digest#sha256:}"
      ;;
    *)
      return 1
      ;;
  esac
}

release_asset_exists() {
  asset="$1"

  release_asset_digest_or_empty "$asset" >/dev/null 2>&1
}

release_asset_digest() {
  asset="$1"

  digest="$(release_asset_digest_or_empty "$asset" || true)"
  if [ -z "$digest" ]; then
    die "could not find SHA-256 digest for release asset $asset."
  fi

  printf '%s\n' "$digest"
}

package_archive_digest() {
  asset="$1"
  manifest_path="$2"

  digest="$(awk -v asset="$asset" '
    $2 == asset && length($1) == 64 && $1 !~ /[^0-9a-fA-F]/ {
      print tolower($1)
      found = 1
      exit
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' "$manifest_path" 2>/dev/null || true)"

  if [ -z "$digest" ]; then
    die "could not find SHA-256 digest for $asset in $CHECKSUM_ASSET."
  fi

  printf '%s\n' "$digest"
}

file_sha256() {
  path="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
    return
  fi

  if command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$path" | sed 's/^.*= //'
    return
  fi

  die "sha256sum, shasum, or openssl is required to verify the download."
}

verify_archive_digest() {
  archive_path="$1"
  expected_digest="$2"
  actual_digest="$(file_sha256 "$archive_path")"

  if [ "$actual_digest" != "$expected_digest" ]; then
    die "downloaded archive checksum mismatch for $(basename "$archive_path").
expected: $expected_digest
actual:   $actual_digest"
  fi
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    die "$1 is required to install codex-vl."
  fi
}

prompt_yes_no() {
  prompt="$1"

  case "$NON_INTERACTIVE" in
    1 | [Tt][Rr][Uu][Ee] | [Yy][Ee][Ss])
      return 1
      ;;
  esac

  if ( : </dev/tty ) 2>/dev/null; then
    printf '%s [y/N] ' "$prompt" >/dev/tty
    if ! IFS= read -r answer </dev/tty; then
      return 1
    fi
  elif [ -t 0 ]; then
    printf '%s [y/N] ' "$prompt"
    if ! IFS= read -r answer; then
      return 1
    fi
  else
    return 1
  fi

  case "$answer" in
    y | Y | yes | YES)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

pick_profile() {
  # Use the same shell-specific split Homebrew documents because there is no
  # universal startup file across macOS/Linux login and interactive shells.
  case "$os:${SHELL:-}" in
    darwin:*/zsh)
      printf '%s\n' "$HOME/.zprofile"
      ;;
    darwin:*/bash)
      printf '%s\n' "$HOME/.bash_profile"
      ;;
    linux:*/zsh)
      printf '%s\n' "$HOME/.zshrc"
      ;;
    linux:*/bash)
      printf '%s\n' "$HOME/.bashrc"
      ;;
    *)
      printf '%s\n' "$HOME/.profile"
      ;;
  esac
}

add_to_path() {
  path_action="already"
  path_profile=""

  case ":$PATH:" in
    *":$BIN_DIR:"*)
      return
      ;;
  esac

  if [ "$MODIFY_PATH" != "1" ]; then
    path_action="skipped"
    return
  fi

  profile="$(pick_profile)"
  path_profile="$profile"
  begin_marker="# >>> codex-vl installer >>>"
  end_marker="# <<< codex-vl installer <<<"
  path_line="export PATH=\"$BIN_DIR:\$PATH\""

  if [ -f "$profile" ] && grep -F "$begin_marker" "$profile" >/dev/null 2>&1; then
    if grep -F "$path_line" "$profile" >/dev/null 2>&1; then
      path_action="configured"
      return
    fi

    if grep -F "$end_marker" "$profile" >/dev/null 2>&1; then
      rewrite_path_block "$profile" "$begin_marker" "$end_marker" "$path_line"
      path_action="updated"
      return
    fi
  fi

  append_path_block "$profile" "$begin_marker" "$end_marker" "$path_line"
  path_action="added"
}

append_path_block() {
  profile="$1"
  begin_marker="$2"
  end_marker="$3"
  path_line="$4"

  {
    printf '\n%s\n' "$begin_marker"
    printf '%s\n' "$path_line"
    printf '%s\n' "$end_marker"
  } >>"$profile"
}

rewrite_path_block() {
  profile="$1"
  begin_marker="$2"
  end_marker="$3"
  path_line="$4"
  tmp_profile="$tmp_dir/profile.$$.tmp"

  awk -v begin="$begin_marker" -v end="$end_marker" -v line="$path_line" '
    BEGIN {
      in_block = 0
      replaced = 0
    }
    $0 == begin {
      if (!replaced) {
        print begin
        print line
        print end
        replaced = 1
      }
      in_block = 1
      next
    }
    in_block {
      if ($0 == end) {
        in_block = 0
      }
      next
    }
    {
      print
    }
    END {
      if (in_block != 0) {
        exit 1
      }
    }
  ' "$profile" >"$tmp_profile"
  mv "$tmp_profile" "$profile"
}

mkdir_lock_is_stale() {
  [ -d "$LOCK_DIR" ] || return 1

  pid="$(cat "$LOCK_DIR/pid" 2>/dev/null || true)"
  started_at="$(cat "$LOCK_DIR/started_at" 2>/dev/null || true)"
  now="$(date +%s 2>/dev/null || printf '0')"

  case "$started_at" in
    ''|*[!0-9]*)
      started_at=0
      ;;
  esac

  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    return 1
  fi

  if [ "$started_at" -eq 0 ] || [ "$now" -eq 0 ]; then
    return 0
  fi

  [ $((now - started_at)) -ge "$LOCK_STALE_AFTER_SECS" ]
}

acquire_install_lock() {
  mkdir -p "$STANDALONE_ROOT"

  if [ "$os" = "darwin" ] && command -v lockf >/dev/null 2>&1; then
    : >>"$LOCK_FILE"
    exec 9<>"$LOCK_FILE"
    lockf 9
    lock_kind="lockf"
    return
  fi

  if command -v flock >/dev/null 2>&1; then
    exec 9>"$LOCK_FILE"
    flock 9
    lock_kind="flock"
    return
  fi

  while ! mkdir "$LOCK_DIR" 2>/dev/null; do
    if mkdir_lock_is_stale; then
      warn "removing stale installer lock at $LOCK_DIR"
      rm -rf "$LOCK_DIR"
      continue
    fi
    sleep 1
  done

  printf '%s\n' "$$" >"$LOCK_DIR/pid"
  date +%s >"$LOCK_DIR/started_at" 2>/dev/null || true
  lock_kind="mkdir"
}

release_install_lock() {
  if [ "$lock_kind" = "mkdir" ]; then
    rm -rf "$LOCK_DIR" 2>/dev/null || true
  elif [ "$lock_kind" = "flock" ] || [ "$lock_kind" = "lockf" ]; then
    exec 9>&- 2>/dev/null || true
  fi
  lock_kind=""
}

cleanup_stale_install_artifacts() {
  mkdir -p "$RELEASES_DIR" "$STANDALONE_ROOT"

  find "$RELEASES_DIR" -mindepth 1 -maxdepth 1 -name '.staging.*' -exec rm -rf {} +
  find "$STANDALONE_ROOT" -mindepth 1 -maxdepth 1 -name '.current.*' -exec rm -f {} +

  if [ -d "$BIN_DIR" ]; then
    find "$BIN_DIR" -mindepth 1 -maxdepth 1 -name '.codex-vl.*' -exec rm -f {} +
  fi
}

record_standalone_releases_in_use() {
  # /proc also catches old clients with no session records. On macOS lsof
  # checks every open file in the release, including running helper programs.
  if [ "$os" = "linux" ]; then
    [ -r /proc/self/mountinfo ] && [ -L /proc/self/exe ] || return 1
    # hidepid can omit other users' processes entirely; do not assume an
    # incomplete process listing proves an older release is unused.
    if grep -E '(^|[ ,])hidepid=[^0, ]' /proc/self/mountinfo >/dev/null; then
      return 1
    fi
    : > "$tmp_dir/in-use" || return 1
    for process in /proc/[0-9]*; do
      if executable="$(readlink "$process/exe" 2>/dev/null)"; then
        printf 'n%s\n' "$executable" >> "$tmp_dir/in-use" || return 1
      elif [ -d "$process" ]; then
        # Even same-user ssh-agent/systemd processes may deny exe inspection.
        # Only an unreadable codex-vl or bundled helper prevents safe pruning.
        process_name="$(cat "$process/comm" 2>/dev/null)" || {
          [ ! -d "$process" ] && continue
          return 1
        }
        # Linux truncates comm to 15 bytes, including the code-mode host.
        case "$process_name" in
          codex* | rg | bwrap)
            # A zombie has no files left open, even if its parent never reaps it.
            grep -Eq '^State:[[:space:]]*Z' "$process/status" 2>/dev/null && continue
            [ ! -d "$process" ] && continue
            return 1
            ;;
        esac
      fi
    done
  else
    # macOS lsof silently omits other users' processes, even with no errors.
    # ps can still report their executable names. As on Linux, decline to
    # prune when an uninspectable codex-vl or helper may be using a release.
    current_uid="$(id -u)" || return 1
    ps -axo uid=,comm= > "$tmp_dir/processes" || return 1
    [ -s "$tmp_dir/processes" ] || return 1
    while read -r process_uid process_name; do
      [ "$process_uid" = "$current_uid" ] && continue
      case "${process_name##*/}" in
        codex* | rg | bwrap) return 1 ;;
      esac
    done < "$tmp_dir/processes"
    if /usr/sbin/lsof -nP -F n +D "$releases" > "$tmp_dir/in-use" 2> "$tmp_dir/lsof-error"; then
      :
    else
      # macOS lsof +D can return 1 with matches when some files are not open.
      # Keep those matches, but reject other failures and any stderr below.
      [ "$?" -eq 1 ] || return 1
    fi
    [ ! -s "$tmp_dir/lsof-error" ] || return 1
  fi
}

release_is_unused() {
  if [ "$os" = "linux" ]; then
    path_field="n$1"
  else
    # lsof +D scopes the scan to this tree, but may escape bytes in CODEX_HOME.
    # Match the installer-owned ASCII path, including the temporary retirement name.
    path_field="/packages/${STANDALONE_ROOT##*/}/releases/${1#"$releases"/}"
  fi
  # Appending / also matches a process whose cwd is the release root itself.
  LC_ALL=C RELEASE_PATH="$path_field/" awk 'index($0 "/", ENVIRON["RELEASE_PATH"]) { exit 1 }' "$tmp_dir/in-use"
}

# Run under the install lock, after the visible command has been verified.
# Only retire ordinary, manifest-owned packages; never retry a retired tree.
prune_standalone_releases() (
  [ ! -L "$CODEX_HOME_DIR/packages" ] &&
    [ ! -L "$STANDALONE_ROOT" ] && [ ! -L "$RELEASES_DIR" ] || return 1
  releases="$(cd -P "$RELEASES_DIR" && pwd)" || return 1
  # Local process inspection cannot establish whether a network home is in
  # use on another host. df -l excludes remotely mounted filesystems.
  df -lP "$releases" > /dev/null 2> "$tmp_dir/df-error" || return 1
  [ ! -s "$tmp_dir/df-error" ] || return 1
  selected="$(cd -P "$CURRENT_LINK" && pwd)" || return 1
  history="$(cat "$STANDALONE_ROOT/selection-history")" || return 1
  previous="$(printf '%s\n' "$history" | sed -n '2p')"
  case "$previous" in
    */* | . | ..) return 1 ;;
  esac
  identity="$(LC_ALL=C stat -c '%d %i %.9Z' "$CURRENT_LINK" 2>/dev/null ||
    LC_ALL=C stat -f '%d %i %Fc' "$CURRENT_LINK")" || return 1
  [ "$selected" = "$releases/${selected##*/}" ] &&
    [ "$history" = "$(printf '%s\n%s\n%s' "${selected##*/}" "$previous" "$identity")" ] ||
    return 1
  record_standalone_releases_in_use || return 1
  restore_candidate=""
  # A stopped install must not strand a late-starting release in retirement.
  trap 'if [ -n "$restore_candidate" ] && [ -d "$retired/release" ] &&
    [ ! -e "$candidate" ] && [ ! -L "$candidate" ]; then
    mv "$retired/release" "$candidate" && rmdir "$retired"
  fi' EXIT
  trap 'exit 1' HUP INT TERM

  for candidate in "$releases"/*; do
    [ -d "$candidate" ] && [ ! -L "$candidate" ] &&
      [ "$candidate" != "$selected" ] && [ "${candidate##*/}" != "$previous" ] || continue
    release_is_unused "$candidate" || continue
    [ -f "$candidate/codex-package.json" ] && [ ! -L "$candidate/codex-package.json" ] || continue
    metadata="$(parse_release_metadata < "$candidate/codex-package.json")" || continue
    version="$(printf '%s\n' "$metadata" | awk -F '\t' '$1 == "version" {print $2}')"
    target="$(printf '%s\n' "$metadata" | awk -F '\t' '$1 == "target" {print $2}')"
    entrypoint="$(printf '%s\n' "$metadata" | awk -F '\t' '$1 == "entrypoint" {print $2}')"
    [ "${candidate##*/}" = "$version-$target" ] && [ "$version" != "latest" ] &&
      [ "$entrypoint" = "bin/codex" ] && [ -f "$candidate/bin/codex" ] || continue
    validate_version "$version" >/dev/null 2>&1 || continue
    case "$target" in
      aarch64-apple-darwin | x86_64-apple-darwin | aarch64-unknown-linux-musl | x86_64-unknown-linux-musl | aarch64-linux-android) ;;
      *) continue ;;
    esac

    # Reject symlinks (except the installer's own alias), special files and
    # mounts before rm -r. find -xdev alone would not protect nested mounts.
    [ "$(readlink "$candidate/codex" 2>/dev/null)" = "bin/codex" ] || continue
    if ! find "$candidate" -xdev ! -type f ! -type d ! -path "$candidate/codex" -print > "$tmp_dir/unowned"; then
      continue
    fi
    [ ! -s "$tmp_dir/unowned" ] || continue
    if [ "$os" = "linux" ]; then
      # mountinfo escapes spaces, tabs, newlines, and backslashes in paths.
      if ! CODEX_PRUNE_RELEASE="$candidate" LC_ALL=C awk '
        BEGIN { release = ENVIRON["CODEX_PRUNE_RELEASE"] }
        { path = $5; gsub(/\\040/, " ", path); gsub(/\\011/, "\t", path)
          gsub(/\\012/, "\n", path); gsub(/\\134/, "\\", path)
          if (path == release || index(path, release "/") == 1) found = 1 }
        END { exit found ? 1 : 0 }
      ' /proc/self/mountinfo; then
        continue
      fi
    else
      device="$(stat -f '%d' "$releases")" || return 1
      if ! find "$candidate" -xdev -exec stat -f '%d' {} + > "$tmp_dir/devices"; then
        continue
      fi
      grep -vxF "$device" "$tmp_dir/devices" >/dev/null && continue
    fi

    retired="$(mktemp -d "$releases/.retired.XXXXXXXX")" || return 1
    restore_candidate=1
    if mv "$candidate" "$retired/release"; then
      # Block new launches at the original path before the final use check.
      # Some systems report the old executable path, so check both names.
      if ! record_standalone_releases_in_use ||
        ! release_is_unused "$candidate" || ! release_is_unused "$retired/release"; then
        [ ! -e "$candidate" ] && [ ! -L "$candidate" ] || return 1
        mv "$retired/release" "$candidate" && rmdir "$retired" || return 1
        restore_candidate=""
        continue
      fi
      # Once proved unused, never restore a possibly partially deleted tree.
      restore_candidate=""
      find "$retired" -xdev -depth -delete || return 1
    else
      restore_candidate=""
      rmdir "$retired"
      return 1
    fi
  done
)

replace_path_with_symlink() {
  link_path="$1"
  link_target="$2"
  tmp_link="$3"

  rm -f "$tmp_link"
  ln -s "$link_target" "$tmp_link"

  if mv -Tf "$tmp_link" "$link_path" 2>/dev/null; then
    return
  fi

  if mv -hf "$tmp_link" "$link_path" 2>/dev/null; then
    return
  fi

  rm -f "$link_path"
  mv -f "$tmp_link" "$link_path"
}

version_from_binary() {
  codex_path="$1"

  if [ ! -x "$codex_path" ]; then
    return 1
  fi

  "$codex_path" --version 2>/dev/null | sed -n 's/.* \([0-9][0-9A-Za-z.+-]*\)$/\1/p' | head -n 1
}

current_installed_version() {
  version="$(version_from_binary "$CURRENT_LINK/bin/codex" || true)"
  if [ -n "$version" ]; then
    printf '%s\n' "$version"
    return 0
  fi
  return 0
}

version_matches_release() {
  installed_version="$1"
  expected_version="$2"

  if [ "$installed_version" = "$expected_version" ]; then
    return 0
  fi

  # The packaged binary reports the crate version (x.y.z) while the release
  # version carries the fork packaging suffix (x.y.z-vl.N): accept the exact
  # match or the crate version as a prefix of the release version.
  case "$expected_version" in
    "$installed_version"-*) return 0 ;;
    *) return 1 ;;
  esac
}

detect_conflicting_command() {
  command_path="$(command -v "$COMMAND_NAME" 2>/dev/null || true)"

  if [ -z "$command_path" ] || [ "$command_path" = "$BIN_PATH" ]; then
    return
  fi

  conflict_path="$command_path"
}

handle_conflicting_command() {
  if [ -z "$conflict_path" ]; then
    return
  fi

  warn "another $COMMAND_NAME was found at $conflict_path."
  warn "PATH order will decide which one runs; nothing was uninstalled."
  case "$NON_INTERACTIVE" in
    1 | [Tt][Rr][Uu][Ee] | [Yy][Ee][Ss])
      # Non-interactive installs continue past a PATH-order warning.
      ;;
    *)
      if ! prompt_yes_no "Continue the standalone install anyway?"; then
        die "aborted; the existing $COMMAND_NAME at $conflict_path was left untouched."
      fi
      ;;
  esac
}

install_package_release() {
  release_dir="$1"
  archive_path="$2"
  stage_release="$RELEASES_DIR/.staging.$(basename "$release_dir").$$"

  mkdir -p "$RELEASES_DIR"
  rm -rf "$stage_release"
  mkdir -p "$stage_release"
  tar -xzf "$archive_path" -C "$stage_release"
  chmod 0755 "$stage_release/bin/codex" "$stage_release/bin/codex-code-mode-host"
  if [ -f "$stage_release/codex-path/rg" ]; then
    chmod 0755 "$stage_release/codex-path/rg"
  fi
  if [ -f "$stage_release/codex-resources/bwrap" ]; then
    chmod 0755 "$stage_release/codex-resources/bwrap"
  fi
  ln -sf "bin/codex" "$stage_release/codex"

  if [ -e "$release_dir" ] || [ -L "$release_dir" ]; then
    rm -rf "$release_dir"
  fi
  mv "$stage_release" "$release_dir"
}

check_target_payload() {
  release_dir="$1"

  [ -f "$release_dir/codex-package.json" ] || die "package archive is missing codex-package.json."
  [ -x "$release_dir/bin/codex" ] || die "package archive is missing an executable bin/codex."
  # The code-mode host is fail-closed since 0.147: refuse the archive without it.
  [ -x "$release_dir/bin/codex-code-mode-host" ] ||
    die "package archive is missing an executable bin/codex-code-mode-host."

  case "$vendor_target" in
    aarch64-linux-android)
      [ -f "$release_dir/bin/libc++_shared.so" ] ||
        die "Android package archive is missing bin/libc++_shared.so."
      if [ ! -x "$release_dir/codex-path/rg" ]; then
        warn "the Android package does not bundle rg; run: pkg install ripgrep"
      fi
      ;;
    x86_64-unknown-linux-musl | aarch64-unknown-linux-musl)
      [ -x "$release_dir/codex-path/rg" ] ||
        die "Linux package archive is missing an executable codex-path/rg."
      [ -x "$release_dir/codex-resources/bwrap" ] ||
        die "Linux package archive is missing an executable codex-resources/bwrap."
      ;;
  esac
}

release_dir_is_complete() {
  release_dir="$1"
  expected_version="$2"

  [ -d "$release_dir" ] &&
    [ "$(basename "$release_dir")" = "$expected_version-$vendor_target" ] &&
    [ -f "$release_dir/codex-package.json" ] &&
    [ -x "$release_dir/bin/codex" ] &&
    [ -x "$release_dir/bin/codex-code-mode-host" ] &&
    [ -x "$release_dir/codex" ] || return 1

  installed_version="$(version_from_binary "$release_dir/bin/codex" || true)"
  version_matches_release "$installed_version" "$expected_version"
}

update_current_link() {
  release_dir="$1"
  tmp_link="$STANDALONE_ROOT/.current.$$"

  replace_path_with_symlink "$CURRENT_LINK" "$release_dir" "$tmp_link"
}

update_visible_command() {
  release_dir="$1"
  mkdir -p "$BIN_DIR"
  tmp_link="$BIN_DIR/.codex-vl.$$"

  replace_path_with_symlink "$BIN_PATH" "$CURRENT_LINK/bin/codex" "$tmp_link"
}

verify_visible_command() {
  "$BIN_PATH" --version >/dev/null
}

print_launch_instructions() {
  case "$path_action" in
    added | updated | configured)
      step "Current terminal: export PATH=\"$BIN_DIR:\$PATH\" && $COMMAND_NAME"
      step "Future terminals: open a new terminal and run: $COMMAND_NAME"
      step "PATH was configured in $path_profile"
      ;;
    skipped)
      step "Run: $BIN_DIR/$COMMAND_NAME"
      ;;
    *)
      step "Run: $COMMAND_NAME"
      ;;
  esac
}

uninstall() {
  if [ ! -d "$STANDALONE_ROOT" ] && [ ! -L "$BIN_PATH" ]; then
    echo "No standalone $COMMAND_NAME install found at $STANDALONE_ROOT."
    exit 0
  fi

  case "$NON_INTERACTIVE" in
    1 | [Tt][Rr][Uu][Ee] | [Yy][Ee][Ss])
      # Explicit non-interactive uninstall proceeds without a prompt.
      ;;
    *)
      if ! prompt_yes_no "Remove $STANDALONE_ROOT and the $BIN_PATH symlink?"; then
        die "aborted; nothing was removed."
      fi
      ;;
  esac

  step "Removing $STANDALONE_ROOT"
  rm -rf "$STANDALONE_ROOT"
  if [ -L "$BIN_PATH" ] || [ -e "$BIN_PATH" ]; then
    step "Removing $BIN_PATH"
    rm -f "$BIN_PATH"
  fi

  echo "Standalone $COMMAND_NAME removed."
  echo "Data under $CODEX_HOME_DIR (settings, sessions) was left in place."
  echo "The official codex command and ~/.codex/packages/standalone were never touched."
  exit 0
}

detect_platform() {
  kernel_name="$(uname -s)"
  kernel_org="$(uname -o 2>/dev/null || true)"
  arch_name="$(uname -m)"

  case "$kernel_name" in
    Darwin)
      os="darwin"
      ;;
    Linux)
      os="linux"
      ;;
    *)
      die "this installer supports Linux and Android (Termux). On Windows use install.ps1."
      ;;
  esac

  # Termux/Android detection: any of these is enough to select the Android
  # target instead of a static musl build (which cannot resolve DNS on
  # Android).
  case "${PREFIX:-}" in
    /data/data/com.termux*) termux_prefix="1" ;;
    *) termux_prefix="" ;;
  esac
  if [ "$kernel_org" = "Android" ] ||
    [ -n "${TERMUX_VERSION:-}" ] || [ "$termux_prefix" = "1" ]; then
    os="android"
  fi

  case "$arch_name" in
    x86_64 | amd64)
      arch="x86_64"
      ;;
    arm64 | aarch64)
      arch="aarch64"
      ;;
    *)
      die "unsupported architecture: $arch_name"
      ;;
  esac

  if [ "$os" = "darwin" ]; then
    if [ "$arch" = "x86_64" ] &&
      [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = "1" ]; then
      arch="aarch64"
    fi
    cat >&2 <<EOF
error: codex-vl does not publish prebuilt macOS binaries yet.

Install on macOS from source with npm instead:

  npm install -g $NPM_PACKAGE@latest --allow-scripts=$NPM_PACKAGE --foreground-scripts

The macOS npm package builds the native binary locally (Xcode Command Line
Tools and Cargo are required). Nothing was installed or modified.
EOF
    exit 1
  fi

  if [ "$os" = "android" ]; then
    if [ "$arch" != "aarch64" ]; then
      die "Android builds are published for arm64 only; found $arch_name."
    fi
    vendor_target="aarch64-linux-android"
    platform_label="Android (Termux, arm64)"
  elif [ "$arch" = "aarch64" ]; then
    vendor_target="aarch64-unknown-linux-musl"
    platform_label="Linux (ARM64, musl static)"
  else
    vendor_target="x86_64-unknown-linux-musl"
    platform_label="Linux (x64, musl static)"
  fi
}

resolve_release() {
  normalized_version="$(normalize_version "$RELEASE")"
  validate_version "$normalized_version"

  if [ -n "$EMBEDDED_RELEASE_VERSION" ]; then
    # Release-pinned installer variant: the digests shipped inside this script
    # are authoritative for its own release; no GitHub API access is needed.
    case "$normalized_version" in
      latest | "$EMBEDDED_RELEASE_VERSION") ;;
      *)
        die "this installer is pinned to $EMBEDDED_RELEASE_VERSION. Unset CODEX_RELEASE or fetch the generic installer for other versions."
        ;;
    esac
    resolved_version="$EMBEDDED_RELEASE_VERSION"
    expected_digest="$(embedded_package_digest "$vendor_target" || true)"
    if [ -z "$expected_digest" ]; then
      die "this installer has no embedded digest for $vendor_target."
    fi
    return
  fi

  if [ -n "$ASSETS_DIR" ]; then
    # Local-asset mode (testing aid): the manifest and archive are read from a
    # directory; only the manifest -> archive leg of the chain applies.
    if [ "$normalized_version" = "latest" ]; then
      die "--from-dir requires an explicit --release VERSION."
    fi
    resolved_version="$normalized_version"
    return
  fi

  if [ "$normalized_version" = "latest" ]; then
    requested_release="latest"
    metadata_url="https://api.github.com/repos/$GITHUB_REPO/releases/latest"
  else
    resolved_version="$normalized_version"
    requested_release="$resolved_version"
    metadata_url="$(release_metadata_url "$resolved_version")"
  fi

  if ! release_json="$(download_text "$metadata_url")"; then
    echo "Could not fetch GitHub release metadata for codex-vl $requested_release." >&2
    echo "The GitHub API may be rate limited (60 anonymous requests/hour)." >&2
    echo "Pin a version to skip the API: CODEX_RELEASE=<version> ./install.sh" >&2
    exit 1
  fi

  if ! release_metadata="$(printf '%s\n' "$release_json" | parse_release_metadata)"; then
    die "could not parse GitHub release metadata for codex-vl $requested_release."
  fi

  if [ "$normalized_version" = "latest" ]; then
    release_tag="$(printf '%s\n' "$release_metadata" | awk -F '\t' '$1 == "tag_name" { print $2; exit }')"
    case "$release_tag" in
      v*) resolved_version="${release_tag#v}" ;;
      *) resolved_version="" ;;
    esac
    if [ -z "$resolved_version" ]; then
      die "failed to resolve the latest codex-vl release version."
    fi
    validate_version "$resolved_version"
  fi

  asset="$PACKAGE_PREFIX$vendor_target.tar.gz"
  if ! release_asset_exists "$asset" || ! release_asset_exists "$CHECKSUM_ASSET"; then
    die "release v$resolved_version does not publish $asset and $CHECKSUM_ASSET."
  fi
}

parse_args "$@"

require_command mktemp
require_command tar

if [ "$(id -u)" = "0" ]; then
  die "refusing to install as root; run this script as your normal user."
fi
if [ -n "${SUDO_USER:-}" ]; then
  die "refusing to install under sudo; run this script as your normal user."
fi

detect_platform

if [ -z "$BIN_DIR" ]; then
  if [ "$os" = "android" ] && [ -n "${PREFIX:-}" ]; then
    BIN_DIR="$PREFIX/bin"
  else
    BIN_DIR="$HOME/.local/bin"
  fi
fi
BIN_PATH="$BIN_DIR/$COMMAND_NAME"

if [ "$UNINSTALL" = "1" ]; then
  uninstall
fi

tmp_dir="$(mktemp -d)"
cleanup() {
  release_install_lock
  if [ -n "$tmp_dir" ]; then
    rm -rf "$tmp_dir"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

resolve_release
asset="$PACKAGE_PREFIX$vendor_target.tar.gz"
release_name="$resolved_version-$vendor_target"
release_dir="$RELEASES_DIR/$release_name"
current_version="$(current_installed_version)"

if [ -n "$current_version" ] &&
  ! version_matches_release "$current_version" "$resolved_version"; then
  step "Updating $COMMAND_NAME from $current_version to $resolved_version"
elif [ -n "$current_version" ]; then
  step "Reinstalling $COMMAND_NAME $resolved_version"
else
  step "Installing $COMMAND_NAME"
fi
step "Detected platform: $platform_label"
step "Resolved version: $resolved_version"

detect_conflicting_command

acquire_install_lock
cleanup_stale_install_artifacts

if ! release_dir_is_complete "$release_dir" "$resolved_version"; then
  if [ -e "$release_dir" ] || [ -L "$release_dir" ]; then
    warn "found incomplete existing release at $release_dir; reinstalling."
  fi

  if [ -n "$ASSETS_DIR" ]; then
    local_archive="$ASSETS_DIR/$asset"
    if [ ! -f "$local_archive" ]; then
      die "$local_archive not found in --from-dir $ASSETS_DIR."
    fi
    sums_path="$ASSETS_DIR/$CHECKSUM_ASSET"
    if [ -f "$sums_path" ]; then
      expected_digest="$(package_archive_digest "$asset" "$sums_path")"
      verify_archive_digest "$local_archive" "$expected_digest"
    else
      warn "no $CHECKSUM_ASSET in $ASSETS_DIR; skipping the manifest digest check."
    fi
    step "Installing from local assets in $ASSETS_DIR"
    install_package_release "$release_dir" "$local_archive"
  else
    step "Downloading $asset"
    archive_path="$tmp_dir/$asset"
    if [ -n "$EMBEDDED_RELEASE_VERSION" ]; then
      download_file "$(release_url_for_asset "$asset" "$resolved_version")" "$archive_path"
      verify_archive_digest "$archive_path" "$expected_digest"
    else
      checksum_path="$tmp_dir/$CHECKSUM_ASSET"
      checksum_digest="$(release_asset_digest "$CHECKSUM_ASSET")"
      download_file "$(release_url_for_asset "$CHECKSUM_ASSET" "$resolved_version")" "$checksum_path"
      verify_archive_digest "$checksum_path" "$checksum_digest"
      expected_digest="$(package_archive_digest "$asset" "$checksum_path")"
      download_file "$(release_url_for_asset "$asset" "$resolved_version")" "$archive_path"
      verify_archive_digest "$archive_path" "$expected_digest"
    fi
    step "Installing standalone package to $release_dir"
    install_package_release "$release_dir" "$archive_path"
  fi
fi

if ! release_dir_is_complete "$release_dir" "$resolved_version"; then
  installed_version="$(version_from_binary "$release_dir/bin/codex" || true)"
  die "installed $COMMAND_NAME reported '$installed_version' instead of $resolved_version."
fi

check_target_payload "$release_dir"

# A no-op must not replace the selection link: history is bound to it.
selected_dir="$(cd -P "$release_dir" && pwd)"
previous_dir="$(cd -P "$CURRENT_LINK" 2>/dev/null && pwd)" || previous_dir=""
if [ "$previous_dir" != "$selected_dir" ]; then
  update_current_link "$release_dir"
  if ! (
    history="$STANDALONE_ROOT/selection-history"
    # GNU and BSD stat both inspect the link itself by default.
    selection_id="$(LC_ALL=C stat -c '%d %i %.9Z' "$CURRENT_LINK" 2>/dev/null ||
      LC_ALL=C stat -f '%d %i %Fc' "$CURRENT_LINK")"
    printf '%s\n%s\n%s\n' "$release_name" "${previous_dir##*/}" "$selection_id" > "$history.tmp.$$"
    mv -f "$history.tmp.$$" "$history"
  ); then
    warn "could not record installation selection; pruning will be skipped."
  fi
fi

update_visible_command "$release_dir"
add_to_path
verify_visible_command
if ! prune_standalone_releases; then
  warn "could not safely inspect old releases; skipped remaining cleanup."
fi
release_install_lock
handle_conflicting_command

print_launch_instructions

printf '%s %s installed successfully.\n' "$COMMAND_NAME" "$resolved_version"
printf 'The official codex command and %s/packages/standalone were not touched.\n' "$CODEX_HOME_DIR"
