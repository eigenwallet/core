{ pkgs ? import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/refs/heads/nixos-25.05.tar.gz";
  }) { }
}:

let
  # monero-depends/hosts/linux.mk hardcodes the Debian-style triple
  # `x86_64-linux-gnu-<tool>` when the build host is x86. Nixpkgs ships
  # those tools under `x86_64-unknown-linux-gnu-*` (or unprefixed), so
  # without these wrappers `configure` reports "C compiler cannot create
  # executables" even though the toolchain is fine.
  prefixWrapper = from: tool: pkgs.writeShellScriptBin "x86_64-linux-gnu-${tool}"
    ''exec ${from}/bin/${tool} "$@"'';

  moneroDependsToolchain = pkgs.symlinkJoin {
    name = "monero-depends-toolchain";
    paths =
      map (prefixWrapper pkgs.gcc) [ "gcc" "g++" "cpp" "cc" ]
      ++ map (prefixWrapper pkgs.binutils)
        [ "ar" "ranlib" "nm" "strip" "ld" "as" "objcopy" "objdump" "readelf" ];
  };

  # Link-time deps for src-tauri and upstream Rust crates. These are found
  # via pkg-config + nix-managed RPATH; do NOT add them to LD_LIBRARY_PATH
  # because that overrides RPATH and can shadow binaries built against a
  # newer openssl (e.g. curl's ngtcp2 module).
  tauriLinkLibs = with pkgs; [
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
  ];

  # Subset that WebKitGTK / GTK dlopen at runtime (tray icon, SVG pixbuf
  # loaders, webview plugins). These must be reachable via LD_LIBRARY_PATH.
  tauriRuntimeLibs = with pkgs; [
    webkitgtk_4_1
    libsoup_3
    librsvg
    libayatana-appindicator
    gdk-pixbuf
  ];
in
pkgs.mkShell {
  # Tools invoked during the build (compilers, codegen, fetchers).
  # Mirrors DEPS_BUILD_LINUX from .github/actions/set-monero-env/action.yml,
  # minus cross-compilation bits (mingw-w64, nsis) that we don't need here.
  nativeBuildInputs = (with pkgs; [
    # C/C++ toolchain for monero-sys (CMake + monero-depends/make)
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

    # Node for src-gui. The project pins `yarn@4.x` via `packageManager` in
    # src-gui/package.json, so we let corepack (bundled with nodejs) fetch
    # that exact version instead of using nixpkgs' yarn 1.x.
    nodejs_22

    # Cargo workspace helpers used by `just` recipes and the CI workflow.
    just
    typeshare
    dprint
    sqlx-cli
    # `just tauri` runs `cargo tauri dev`, which needs the Rust tauri CLI
    # on PATH (provides the `cargo-tauri` subcommand shim). The GitHub CI
    # uses the yarn-based `@tauri-apps/cli` instead, so it doesn't need this.
    cargo-tauri
  ]) ++ [ moneroDependsToolchain ];

  # Native libraries linked by src-tauri and its webkit-based webview.
  buildInputs = tauriLinkLibs;

  # monero-sys build.rs picks up $CC/$CXX; be explicit so we don't accidentally
  # pull in a host toolchain that isn't in $PATH under nix-shell.
  CC = "${pkgs.gcc}/bin/gcc";
  CXX = "${pkgs.gcc}/bin/g++";

  # WebKitGTK dlopens some of its plugins at runtime, so `tauri dev`
  # needs them reachable via LD_LIBRARY_PATH in addition to being linked.
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath tauriRuntimeLibs;

  shellHook = ''
    # Rustup-managed toolchain lives in ~/.cargo/bin; nix-shell resets PATH
    # for its own deps, so re-prepend it. rust-toolchain.toml pins the version.
    export PATH="$HOME/.cargo/bin:$PATH"

    # Let corepack materialise the project-pinned yarn version into a user-
    # writable dir (nodejs bin in /nix/store is read-only, so the default
    # `corepack enable` location fails).
    export COREPACK_HOME="$HOME/.cache/corepack"
    export COREPACK_ENABLE_DOWNLOAD_PROMPT=0
    corepack_bin="$HOME/.cache/corepack/bin"
    mkdir -p "$corepack_bin"
    ${pkgs.nodejs_22}/bin/corepack enable --install-directory "$corepack_bin"
    export PATH="$corepack_bin:$PATH"

    # GSettings/GIO schemas for GTK apps (otherwise Tauri dialogs warn/crash).
    export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS"
  '';
}
