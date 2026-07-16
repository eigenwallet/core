#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
self="$repo_root/nix/macos-isolated-build.sh"
cd "$repo_root"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(cd "$repo_root/.." && pwd)/nixbuild/target}"

if [ -z "${IN_NIX_SHELL:-}" ]; then
  exec nix-shell "$repo_root/shell.nix" --run "$(printf '%q ' "$self" "$@")"
fi

clean=""
IFS=':' read -ra _parts <<<"$PATH"
for _p in "${_parts[@]}"; do
  case "$_p" in
    /opt/homebrew/* | /usr/local/Homebrew/* | /usr/local/bin | /usr/local/sbin | /opt/local/* | */.nvm/*)
      continue
      ;;
  esac
  clean="${clean:+$clean:}$_p"
done
export PATH="$clean"

unset LIBRARY_PATH DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH \
  CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH CMAKE_PREFIX_PATH OPENSSL_DIR 2>/dev/null || true

if command -v brew >/dev/null 2>&1; then
  echo "FATAL: brew is still reachable at $(command -v brew); isolation broken." >&2
  exit 1
fi

echo "[isolated] CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
echo "[isolated] cc=$(command -v cc)  clang=$(command -v clang)  xcrun=$(command -v xcrun)"
echo "[isolated] cmake=$(command -v cmake)  make=$(command -v make)  node=$(command -v node)"
echo "[isolated] running: $*"
exec "$@"
