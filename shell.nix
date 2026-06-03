{ pkgs ? import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/ac62194c3917d5f474c1a844b6fd6da2db95077d.tar.gz";
    sha256 = "0v6bd1xk8a2aal83karlvc853x44dg1n4nk08jg3dajqyy0s98np";
  }) {
    config.allowUnfreePredicate = p:
      builtins.match "nvidia.*" (builtins.parseDrvName p.name).name != null;
  }
, nvidiaVersion ?
    let
      # nix can't readFile /proc directly (NixOS/nix#3539); copy it out impurely first
      versionFile = pkgs.runCommand "impure-nvidia-version" {
        time = builtins.currentTime;
        preferLocalBuild = true;
        allowSubstitutes = false;
      } ''cp /proc/driver/nvidia/version "$out" 2>/dev/null || touch "$out"'';
      firstLine = builtins.head (pkgs.lib.splitString "\n" (builtins.readFile versionFile));
      # "( for <arch>)?" also matches the NVIDIA open kernel module, not just proprietary
      m = builtins.match ".*Kernel Module( for [^ ]+)?  ([0-9.]+)  .*" firstLine;
    in if m == null then null else builtins.elemAt m 1
}:

let
  supportedSystem = pkgs.stdenv.hostPlatform.system == "x86_64-linux";
in
if supportedSystem then
let
  prefixWrapper = from: tool: pkgs.writeShellScriptBin "x86_64-linux-gnu-${tool}"
    ''exec ${from}/bin/${tool} "$@"'';

  moneroDependsToolchain = pkgs.symlinkJoin {
    name = "monero-depends-toolchain";
    paths =
      map (prefixWrapper pkgs.gcc) [ "gcc" "g++" "cpp" "cc" ]
      ++ map (prefixWrapper pkgs.binutils)
        [ "ar" "ranlib" "nm" "strip" "ld" "as" "objcopy" "objdump" "readelf" ];
  };

  nixGLNvidia = (import (builtins.fetchTarball {
    url = "https://github.com/nix-community/nixGL/archive/b6105297e6f0cd041670c3e8628394d4ee247ed5.tar.gz";
    sha256 = "1zv3bshk0l4hfh1s7s3jzwjxl0nqqcvc4a3kydd3d4lgh7651d3x";
  }) { inherit pkgs nvidiaVersion; }).nixGLNvidia;

  gpuShellHook =
    if nvidiaVersion != null then ''
      # strip the trailing exec so sourcing the nixGL wrapper doesn't exec an empty argv
      eval "$(${pkgs.gnused}/bin/sed '/^[[:space:]]*exec /d' ${nixGLNvidia}/bin/nixGLNvidia-${nvidiaVersion})"
      export GDK_BACKEND=x11
    '' else ''
      export __EGL_VENDOR_LIBRARY_DIRS=${pkgs.mesa}/share/glvnd/egl_vendor.d
      export LIBGL_DRIVERS_PATH=${pkgs.mesa}/lib/dri
      export LIBGL_ALWAYS_SOFTWARE=1
      export WEBKIT_DISABLE_DMABUF_RENDERER=1
    '';

  tauriLinkLibs = (with pkgs; [
    glib
    gtk3
    gdk-pixbuf
    cairo
    pango
    atkmm
    at-spi2-atk
    libsoup_3
    webkitgtk_4_1
    librsvg
    libayatana-appindicator
    openssl
    zlib
  ]) ++ [ pkgs.stdenv.cc.cc.lib ];

  tauriRuntimeLibs = with pkgs; [
    webkitgtk_4_1
    libsoup_3
    librsvg
    libayatana-appindicator
    gdk-pixbuf
  ];
in
pkgs.mkShell {
  nativeBuildInputs = (with pkgs; [
    gcc
    gnumake
    cmake
    autoconf
    automake
    libtool
    pkg-config
    binutils
    ccache
    gperf
    lbzip2
    curl
    git
    python3
    nodejs_22
    just
    typeshare
    dprint
    sqlx-cli
    cargo-tauri
  ]) ++ [ moneroDependsToolchain ];

  buildInputs = tauriLinkLibs;

  CC = "${pkgs.gcc}/bin/gcc";
  CXX = "${pkgs.gcc}/bin/g++";

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath tauriRuntimeLibs;

  shellHook = ''
    # nix-shell's default -rpath is $out/lib which never exists — repoint it or every cargo-linked binary gets a dead RUNPATH
    if [ -n "$out" ] && [[ "$NIX_LDFLAGS" == *"-rpath $out/lib"* ]]; then
      _rpath_real=${pkgs.lib.makeLibraryPath tauriLinkLibs}
      export NIX_LDFLAGS="''${NIX_LDFLAGS//-rpath $out\/lib/-rpath $_rpath_real}"
      unset _rpath_real
    fi

    ${gpuShellHook}
    if ! command -v docker >/dev/null 2>&1 && command -v podman >/dev/null 2>&1; then
      _docker_shim="$HOME/.cache/eigenwallet-docker-shim"
      mkdir -p -m 700 "$_docker_shim"
      printf '#!/bin/sh\nexec podman "$@"\n' > "$_docker_shim/docker"
      chmod +x "$_docker_shim/docker"
      export PATH="$PATH:$_docker_shim"
      export TESTCONTAINERS_RYUK_DISABLED=true
      unset _docker_shim
    fi

    export PATH="$HOME/.cargo/bin:$PATH"

    export COREPACK_HOME="$HOME/.cache/corepack"
    export COREPACK_ENABLE_DOWNLOAD_PROMPT=0
    corepack_bin="$HOME/.cache/corepack/bin"
    mkdir -p "$corepack_bin"
    ${pkgs.nodejs_22}/bin/corepack enable --install-directory "$corepack_bin"
    export PATH="$corepack_bin:$PATH"

    export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS"
  '';
}
else
pkgs.mkShell {
  shellHook = ''
    echo "Skipping eigenwallet Nix dev shell; supported only on x86_64 Linux."
  '';
}
