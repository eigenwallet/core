#!/bin/bash

# Script to build gcc cross compiler for windows from source on ubuntu.
# Installed into ~/opt/gcc-mingw-14.3


# Versions
BINUTILS_VER=2.42
MINGW_VER=v12.0.0
GCC_VER=14.3.0

# Some flags for running only certain parts of the script.
# Set these to 1 before running the script to make use of them.
ONLY_WINPTHREADS="${ONLY_WINPTHREADS:-}"
ONLY_COPY_DLLS="${ONLY_COPY_DLLS:-}"
ONLY_VERIFY="${ONLY_VERIFY:-}"

# OS Detection and validation
detected=$(uname -s)-$(uname -m)
case "$detected" in
    Linux-x86_64)
        # Check if it's Ubuntu
        if [ -f /etc/os-release ]; then
            . /etc/os-release
            if [ "$ID" != "ubuntu" ]; then
                echo "This script is designed for ubuntu (x86-64) and doesn't support: ${detected} (${PRETTY_NAME})"
                exit 1
            fi
        else
            echo "This script is designed for ubuntu (x86-64) and doesn't support: ${detected}"
            exit 1
        fi
        ;;
    *)
        echo "This script is designed for ubuntu (x86-64) and doesn't support: ${detected}"
        exit 1
        ;;
esac

set -euo pipefail

# Get the current project root (this file is in <root>/dev_scripts/ and gets called via just (just file is at <root>/justfile))
SRC_TAURI_DIR="$(pwd)/../src-tauri"

# Check if src-tauri directory exists
if [ ! -d "$SRC_TAURI_DIR" ]; then
    echo "Error: must be called from project root -> src-tauri must be subdir"
    echo "Current directory: $(pwd)"
    exit 1
fi

install_deps() {
    # Package installation (idempotent)
    echo "Ensuring required packages are installed"
    to_install=()
    for pkg in gpg build-essential wget flex bison texinfo libgmp-dev libmpfr-dev libmpc-dev libisl-dev zlib1g-dev libbz2-dev libffi-dev python3 gnupg dirmngr ca-certificates jq; do
        if ! dpkg -s "$pkg" >/dev/null 2>&1; then
            echo "missing package: $pkg"
            to_install+=("$pkg")
        fi
    done
    if [ ${#to_install[@]} -gt 0 ]; then
        sudo apt update
        sudo apt install -y "${to_install[@]}"
    else
        echo "All required packages already installed"
    fi
}

export PREFIX=~/opt/gcc-mingw-14.3
export BUILD=$HOME/mingw-build
export SRC=$BUILD/src
mkdir -p $SRC \
         $BUILD/build-binutils \
         $BUILD/build-headers \
         $BUILD/build-gcc \
         $BUILD/build-crt \
         $BUILD/build-winpthreads \
         $PREFIX

cd $SRC

download_if_missing() {
    local url="$1"
    local out="${2:-}"
    local dest
    if [ -n "$out" ]; then
        dest="$out"
    else
        dest="$(basename "$url")"
    fi
    if [ -f "$dest" ]; then
        echo "Already present: $dest"
    else
        echo "Downloading: $url"
        wget -q "$url" -O "$dest"
    fi
}

fetch_gpg_key() {
    # Usage: fetch_gpg_key <keyid_or_fingerprint>
    local key="$1"

    # Try multiple hkps keyservers first
    for ks in hkps://keyserver.ubuntu.com hkps://keys.openpgp.org hkps://pgp.mit.edu; do
        if gpg --keyserver "$ks" --keyserver-options timeout=10 --recv-keys "$key"; then
            return 0
        fi
    done

    # HTTP fallback: Ubuntu keyserver
    if ! gpg --list-keys "$key" >/dev/null 2>&1; then
        if curl -fsSL "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x${key}" | gpg --import; then
            if gpg --list-keys "$key" >/dev/null 2>&1; then
                return 0
            fi
        fi
    fi

    # HTTP fallback: keys.openpgp.org by fingerprint (no 0x)
    if ! gpg --list-keys "$key" >/dev/null 2>&1; then
        local fpr_no0x
        fpr_no0x=$(echo "$key" | sed 's/^0x//')
        curl -fsSL "https://keys.openpgp.org/vks/v1/by-fingerprint/${fpr_no0x}" | gpg --import || true
        if gpg --list-keys "$key" >/dev/null 2>&1; then
            return 0
        fi
    fi

    return 1
}

ensure_key_and_verify() {
    # Usage: ensure_key_and_verify <artifact> <signature>
    local artifact="$1"
    local sig="$2"

    echo "Verifying signature: $sig for $artifact"

    # First attempt: verify with whatever keys are available
    if gpg --batch --status-fd 1 --verify "$sig" "$artifact" 2>verify.stderr | tee verify.status | grep -q "\[GNUPG:\] VALIDSIG"; then
        echo "GPG verification OK for $artifact"
        rm -f verify.status verify.stderr
        return 0
    fi

    # If missing key, try to fetch by fingerprint or keyid
    local missing_key
    missing_key=$(grep "\[GNUPG:\] NO_PUBKEY" verify.status | awk '{print $3}' || true)
    if [ -z "$missing_key" ]; then
        # Try to extract key id from stderr (older gpg formats)
        missing_key=$(grep -Eo 'key [0-9A-Fa-f]+' verify.stderr | awk '{print $2}' | tail -n1 || true)
    fi

    if [ -n "$missing_key" ]; then
        echo "Missing public key: $missing_key. Attempting key fetch."
        fetch_gpg_key "$missing_key" || true
    fi

    # Second attempt: verify again
    if gpg --batch --status-fd 1 --verify "$sig" "$artifact" 2>/dev/null | grep -q "\[GNUPG:\] VALIDSIG"; then
        echo "GPG verification OK for $artifact"
        rm -f verify.status verify.stderr
        return 0
    fi

    echo "ERROR: GPG verification failed for $artifact" >&2
    echo "GPG status:"
    cat verify.status || true
    echo "GPG stderr:"
    cat verify.stderr || true
    rm -f verify.status verify.stderr
    exit 1
}


download_sources() {
    # Binutils
    download_if_missing "https://ftp.gnu.org/gnu/binutils/binutils-${BINUTILS_VER}.tar.xz"
    download_if_missing "https://ftp.gnu.org/gnu/binutils/binutils-${BINUTILS_VER}.tar.xz.sig"
    ensure_key_and_verify "binutils-${BINUTILS_VER}.tar.xz" "binutils-${BINUTILS_VER}.tar.xz.sig"
    tar xf "binutils-${BINUTILS_VER}.tar.xz"

    # mingw-w64 headers & CRT
    download_if_missing "https://sourceforge.net/projects/mingw-w64/files/mingw-w64/mingw-w64-release/mingw-w64-${MINGW_VER}.tar.bz2"
    download_if_missing "https://sourceforge.net/projects/mingw-w64/files/mingw-w64/mingw-w64-release/mingw-w64-${MINGW_VER}.tar.bz2.sig"
    ensure_key_and_verify "mingw-w64-${MINGW_VER}.tar.bz2" "mingw-w64-${MINGW_VER}.tar.bz2.sig"
    tar xf "mingw-w64-${MINGW_VER}.tar.bz2"

    # GCC
    download_if_missing "https://ftp.gnu.org/gnu/gcc/gcc-${GCC_VER}/gcc-${GCC_VER}.tar.xz"
    download_if_missing "https://ftp.gnu.org/gnu/gcc/gcc-${GCC_VER}/gcc-${GCC_VER}.tar.xz.sig"
    ensure_key_and_verify "gcc-${GCC_VER}.tar.xz" "gcc-${GCC_VER}.tar.xz.sig"
    tar xf "gcc-${GCC_VER}.tar.xz"
}


build_binutils() {
    echo "Building binutils"

    cd $BUILD/build-binutils
    $SRC/binutils-${BINUTILS_VER}/configure \
      --target=x86_64-w64-mingw32 \
      --prefix=$PREFIX \
      --with-sysroot=$PREFIX \
      --disable-multilib
    make -j$(nproc)
    make install
    export PATH="$PREFIX/bin:$PATH"

    echo "Built binutils"
}

build_mingw_headers() {
    echo "Building mingw-w64 headers"

    cd $BUILD/build-headers
    $SRC/mingw-w64-${MINGW_VER}/mingw-w64-headers/configure \
      --host=x86_64-w64-mingw32 \
      --prefix=$PREFIX/x86_64-w64-mingw32
    make -j$(nproc)
    make install

    # fixes a path mismatch issue
    if [ ! -L "$PREFIX/mingw" ]; then
        ln -s $PREFIX/x86_64-w64-mingw32 $PREFIX/mingw
    fi

    echo "Built mingw-w64 headers"
}

prepare_gcc_build() {
    echo "Building gcc"

    cd $BUILD/build-gcc
    $SRC/gcc-${GCC_VER}/configure \
      --target=x86_64-w64-mingw32 \
      --prefix=$PREFIX \
      --with-sysroot=$PREFIX \
      --disable-multilib \
      --enable-languages=c,c++
    make all-gcc -j$(nproc)
    make install-gcc

    echo "Built gcc"
}

build_mingw_crt() {
    echo "Building mingw-w64 CRT"

    cd $BUILD/build-crt
    $SRC/mingw-w64-${MINGW_VER}/mingw-w64-crt/configure \
      --host=x86_64-w64-mingw32 \
      --prefix=$PREFIX/x86_64-w64-mingw32 \
      --with-sysroot=$PREFIX
    make -j$(nproc)
    make install

    echo "Built mingw-w64 CRT"
}

finish_gcc() {
    echo "Finishing gcc build"

    cd $BUILD/build-gcc
    make -j$(nproc)
    make install

    # Add to PATH only if not already present
    if [[ ":$PATH:" != *":$PREFIX/bin:"* ]]; then
        export PATH="$PREFIX/bin:$PATH"
    fi

    # add path to bashrc
    if ! grep -q "export PATH=\"$PREFIX/bin:\$PATH\"" ~/.bashrc; then
        echo "export PATH=\"$PREFIX/bin:\$PATH\"" >> ~/.bashrc
    fi

    echo "Built gcc"
}

build_winpthreads() {
    echo "Building winpthreads.dll"

    cd $BUILD/build-winpthreads

    # 2. Configure winpthreads (static & shared)
    $SRC/mingw-w64-${MINGW_VER}/mingw-w64-libraries/winpthreads/configure \
      --host=x86_64-w64-mingw32        \
      --prefix=$PREFIX/x86_64-w64-mingw32 \
      --enable-static --enable-shared  \
      --disable-multilib

    # 3. Build & install
    make -j$(nproc)
    make install

    echo "Built winpthreads.dll"
}

copy_dlls() {
    echo "Copying dll's to src-tauri/"
    cp -f $PREFIX/x86_64-w64-mingw32/lib/{libstdc++-6,libgcc_s_seh-1}.dll $SRC_TAURI_DIR/
}

verify_installation() {
    # 1. check that ~/opt/gcc-mingw-14.3/ exists 
    # 2. check that `x86_64-w64-mingw32-g++ --version` works and the output contains 14.3 
    # 3. make sure the dll's are in the src-tauri directory

	echo "Verifying cross-compiler installation"

	local has_error=0

	# Ensure PREFIX dir exists
	if [ ! -d "$PREFIX" ]; then
		echo "ERROR: Expected install directory not found: $PREFIX"
		has_error=1
	else
		echo "OK: Found install directory $PREFIX"
	fi

	# Ensure the compiler is on PATH for the check
	if ! command -v x86_64-w64-mingw32-g++ >/dev/null 2>&1; then
		export PATH="$PREFIX/bin:$PATH"
	fi

	# Check compiler exists and version matches expected major.minor (e.g., 14.3)
	local gcc_out=""
	local gcc_short_ver
	gcc_short_ver="${GCC_VER%.*}"
	if ! gcc_out="$(x86_64-w64-mingw32-g++ --version 2>/dev/null)"; then
		echo "ERROR: x86_64-w64-mingw32-g++ is not runnable or not found on PATH"
		has_error=1
	else
		if echo "$gcc_out" | grep -q "$gcc_short_ver"; then
			echo "OK: x86_64-w64-mingw32-g++ version contains $gcc_short_ver"
		else
			echo "ERROR: x86_64-w64-mingw32-g++ version does not contain $gcc_short_ver"
			echo "$gcc_out" | head -n1
			has_error=1
		fi
	fi

	# Check DLLs in src-tauri directory
	local missing_dlls=()
	for dll in libstdc++-6.dll libgcc_s_seh-1.dll; do
		if [ ! -f "$SRC_TAURI_DIR/$dll" ]; then
			missing_dlls+=("$dll")
		fi
	done
	if [ ${#missing_dlls[@]} -eq 0 ]; then
		echo "OK: Required DLLs are present in $SRC_TAURI_DIR"
	else
		echo "ERROR: Missing DLLs in $SRC_TAURI_DIR: ${missing_dlls[*]}"
		echo "Hint: run `just prepare-windows-build` to build and copy the required DLLs"
		has_error=1
	fi

	if [ "$has_error" -eq 0 ]; then
		echo "Verification successful"
		return 0
	else
		echo "Verification failed"
		return 1
	fi
}

if [ -n "${ONLY_WINPTHREADS}" ]; then
    echo "ONLY_WINPTHREADS set; building winpthreads.dll"
    install_deps
    download_sources
    build_winpthreads
    exit 0
fi

if [ -n "${ONLY_COPY_DLLS}" ]; then
    echo "ONLY_COPY_DLLS set; copying dll's to src-tauri/"
    copy_dlls
    exit 0
fi

if [ -n "${ONLY_VERIFY}" ]; then
    verify_installation
    exit 0
fi

install_deps
download_sources

build_binutils
build_mingw_headers
prepare_gcc_build
build_mingw_crt
finish_gcc
build_winpthreads
copy_dlls

verify_installation

echo "Done"