#!/usr/bin/env bash
# Runtime compatibility guard for the linux x86_64 AppImage (issue #204,
# https://github.com/eigenwallet/core/issues/204):
#
#   1. no file bundled into the AppImage may require glibc / libstdc++ / C++ ABI
#      symbol versions newer than the Ubuntu 22.04 build floor
#      (GLIBC_2.35 / GLIBCXX_3.4.29 / CXXABI_1.3.13). Anything above breaks the
#      AppImage on Debian 12 at load time ("GLIBC_2.38 not found ...").
#   2. libwayland-client.so.0, libwayland-cursor.so.0, libwayland-egl.so.1 and
#      libwayland-server.so.0 must not be bundled: they shadow the host wayland
#      libraries and break EGL on Fedora ("Could not create default EGL display:
#      EGL_BAD_PARAMETER"). The AppImage uses the system wayland libs instead.
#
# Usage: check-appimage-glibc-compat.sh <path-to-.AppImage>
# Thresholds can be overridden: MAX_GLIBC / MAX_GLIBCXX / MAX_CXXABI.

set -euo pipefail

appimage="${1:?usage: check-appimage-glibc-compat.sh <path-to-.AppImage>}"
[[ -f "$appimage" ]] || { echo "ERROR: '$appimage' does not exist" >&2; exit 1; }
# the caller may pass a path relative to the repo/workflow working directory
appimage="$(readlink -f "$appimage")"

max_glibc="${MAX_GLIBC:-2.35}"
max_glibcxx="${MAX_GLIBCXX:-3.4.29}"
max_cxxabi="${MAX_CXXABI:-1.3.13}"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

# extract (works without FUSE requirements)
chmod +x "$appimage"
(cd "$workdir" && "$appimage" --appimage-extract >/dev/null)
root="$workdir/squashfs-root"
[[ -d "$root" ]] || { echo "ERROR: could not extract '$appimage'" >&2; exit 1; }

echo "Checking '$(basename "$appimage")':
  glibc floor:    <= GLIBC_$max_glibc
  libstdc++ floor:<= GLIBCXX_$max_glibcxx
  C++ ABI floor:  <= CXXABI_$max_cxxabi
  wayland libs:   must not be bundled
"

violations=()

# --- 2. wayland libraries must not be bundled ---
mapfile -t wayland_hits < <(find "$root" \
  \( -name 'libwayland-client.so.0*' -o -name 'libwayland-cursor.so.0*' \
     -o -name 'libwayland-egl.so.*' -o -name 'libwayland-server.so.0*' \) -print)
for hit in "${wayland_hits[@]}"; do
  violations+=("bundled wayland lib (rule 2, use the system's instead): ${hit#"${root}"/}")
done

# --- 1. symbol-version requirements of every bundled ELF must stay <= floor ---
# Only the "Version References:" block of objdump -p (the ELF .gnu.version_r
# section) is inspected: the versions each file requires from libc/libstdc++/
# libgcc at load time. "Version definitions:" (.gnu.version_d, what a library
# itself provides, e.g. libstdc++ defining GLIBCXX_3.4.30) is ignored, since
# providing a version is not requiring it.
requires_max() {
  # "$1" <= "$2" ? 0 : 1
  [[ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -n1)" == "$1" ]]
}

while IFS= read -r -d '' elf; do
  # skip non-ELF files (icons, .desktop, locales, ...)
  [[ "$(head -c 4 "$elf")" == $'\x7fELF' ]] || continue
  while IFS= read -r req; do
    case "$req" in
      GLIBC_*)
        requires_max "${req#GLIBC_}" "$max_glibc" \
          || violations+=("${elf#"${root}"/}: requires $req > GLIBC_$max_glibc (rule 1)") ;;
      GLIBCXX_*)
        requires_max "${req#GLIBCXX_}" "$max_glibcxx" \
          || violations+=("${elf#"${root}"/}: requires $req > GLIBCXX_$max_glibcxx (rule 1)") ;;
      CXXABI_*)
        requires_max "${req#CXXABI_}" "$max_cxxabi" \
          || violations+=("${elf#"${root}"/}: requires $req > CXXABI_$max_cxxabi (rule 1)") ;;
    esac
  done < <(objdump -p "$elf" 2>/dev/null \
    | awk '/^Version References:/{f=1;next} /^Version definitions:/{f=0} f' \
    | grep -oE '(GLIBC|GLIBCXX|CXXABI)_[0-9]([0-9.]*)' | sort -u)
done < <(find "$root" -type f -print0)

if ((${#violations[@]})); then
  echo >&2 "ERROR: '${appimage}' is not runtime compatible (issue #204):"
  printf '%s\n' "${violations[@]}" | sort -u >&2
  exit 1
fi

echo "OK: AppImage complies with the Debian-12 glibc/GLIBCXX/CXXABI floor and does not bundle wayland libs"
