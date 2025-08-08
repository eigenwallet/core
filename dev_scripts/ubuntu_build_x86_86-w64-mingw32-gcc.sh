#!/bin/bash

# Script to build gcc cross compiler for windows from source on ubuntu.
# Installed into ~/opt/gcc-mingw-14.3

#sudo apt update
#sudo apt install -y \
#  build-essential wget flex bison texinfo \
#   libgmp-dev libmpfr-dev libmpc-dev libisl-dev \
#   libz-dev libbz2-dev libffi-dev python3

export PREFIX=~/opt/gcc-mingw-14.3
export BUILD=$HOME/mingw-build
export SRC=$BUILD/src
mkdir -p $SRC \
         $BUILD/build-binutils \
         $BUILD/build-headers \
         $BUILD/build-gcc \
         $BUILD/build-crt \
         $PREFIX

cd $SRC
# Binutils
wget https://ftp.gnu.org/gnu/binutils/binutils-2.42.tar.xz
wget https://ftp.gnu.org/gnu/binutils/binutils-2.42.tar.xz.sig

# Verify Binutils signature; if this script is sourced, avoid exiting the entire shell
if ! gpg --verify --keyring ./gnu-keyring.gpg binutils-2.42.tar.xz.sig binutils-2.42.tar.xz; then
  echo "Error: binutils signature verification failed" >&2
  # 'return' works when the script is sourced; if not, fall back to 'exit'
  return 1 2>/dev/null || exit 1
fi

tar xf binutils-2.42.tar.xz

# mingw-w64 headers & CRT
wget https://sourceforge.net/projects/mingw-w64/files/mingw-w64/mingw-w64-release/mingw-w64-v12.0.0.tar.bz2
wget https://sourceforge.net/projects/mingw-w64/files/mingw-w64/mingw-w64-release/mingw-w64-v12.0.0.tar.bz2.sig

# if ! gpg --verify --keyring ./gnu-keyring.gpg mingw-w64-v12.0.0.tar.bz2.sig mingw-w64-v12.0.0.tar.bz2; then
#   echo "Error: mingw-w64 signature verification failed" >&2
#   return 1 2>/dev/null || exit 1
# fi

echo "Skipping mingw-w64 signature verification - TODO (need to find keyring)"

tar xf mingw-w64-v12.0.0.tar.bz2

# GCC 14.3
wget https://ftp.gnu.org/gnu/gcc/gcc-14.3.0/gcc-14.3.0.tar.xz
wget https://ftp.gnu.org/gnu/gcc/gcc-14.3.0/gcc-14.3.0.tar.xz.sig

# Verify Binutils signature; if this script is sourced, avoid exiting the entire shell
if ! gpg --verify --keyring ./gnu-keyring.gpg binutils-2.42.tar.xz.sig binutils-2.42.tar.xz; then
  echo "Error: binutils signature verification failed" >&2
  # 'return' works when the script is sourced; if not, fall back to 'exit'
  return 1 2>/dev/null || exit 1
fi

tar xf gcc-14.3.0.tar.xz

echo "Building binutils"

cd $BUILD/build-binutils
$SRC/binutils-2.40/configure \
  --target=x86_64-w64-mingw32 \
  --prefix=$PREFIX \
  --with-sysroot=$PREFIX \
  --disable-multilib
make -j$(nproc)
sudo make install
export PATH="$PREFIX/bin:$PATH"

echo "Built binutils"

echo "Building mingw-w64 headers"

cd $BUILD/build-headers
$SRC/mingw-w64-v12.0.0/mingw-w64-headers/configure \
  --host=x86_64-w64-mingw32 \
  --prefix=$PREFIX/x86_64-w64-mingw32
make -j$(nproc)
sudo make install

echo "Built mingw-w64 headers"

echo "Building gcc"

cd $BUILD/build-gcc
$SRC/gcc-14.3.0/configure \
  --target=x86_64-w64-mingw32 \
  --prefix=$PREFIX \
  --with-sysroot=$PREFIX \
  --disable-multilib \
  --enable-languages=c,c++
make all-gcc -j$(nproc)
sudo make install-gcc

echo "Built gcc"

echo "Building mingw-w64 CRT"

cd $BUILD/build-crt
$SRC/mingw-w64-v12.0.0/mingw-w64-crt/configure \
  --host=x86_64-w64-mingw32 \
  --prefix=$PREFIX/x86_64-w64-mingw32 \
  --with-sysroot=$PREFIX
make -j$(nproc)
sudo make install

echo "Built mingw-w64 CRT"

echo "Finishing gcc build"

cd $BUILD/build-gcc
make -j$(nproc)
sudo make install

echo "Built gcc"