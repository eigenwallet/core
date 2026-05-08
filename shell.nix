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
  # via pkg-config and the RPATH baked in by NIX_LDFLAGS (rewritten in
  # shellHook below — see the comment there for why); we deliberately keep
  # them off LD_LIBRARY_PATH so they can't shadow nix-built tools like curl
  # whose ngtcp2 module is pinned to a specific openssl ABI.
  #
  # `stdenv.cc.cc.lib` is included so RUNPATH covers libstdc++.so.6, which
  # the tauri binary pulls in via the C++ webkitgtk/cxx bindings.
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

  # Subset that WebKitGTK / GTK dlopen at runtime (tray icon, SVG pixbuf
  # loaders, webview plugins). dlopen of a bare soname only consults
  # LD_LIBRARY_PATH and the system cache, not the calling binary's RUNPATH,
  # so these have to be on LD_LIBRARY_PATH even though they're also linked.
  #
  # zlib is here as a safety net for cargo build-script binaries (e.g.
  # the vergen-git2 -> libgit2-sys path used by swap, swap-asb,
  # swap-orchestrator). The rpath rewrite in shellHook fixes their
  # RUNPATH for fresh links, but build-script binaries linked before
  # the fix went in still carry a dead RUNPATH and would fail to
  # resolve libz when cargo re-runs them after a `rerun-if-changed`
  # trigger (e.g. a new git commit). Keeping libz on LD_LIBRARY_PATH
  # avoids forcing users to `cargo clean` after picking up shell.nix.
  tauriRuntimeLibs = with pkgs; [
    webkitgtk_4_1
    libsoup_3
    librsvg
    libayatana-appindicator
    gdk-pixbuf
    zlib
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

  # WebKitGTK uses libglvnd (from nixpkgs) for EGL/GLX dispatch, but the
  # nix-shipped libglvnd has no vendor ICD installed by default. On NixOS
  # the system populates `/run/opengl-driver/lib`; on a non-NixOS host
  # there's nothing for libglvnd to dispatch to, so `eglGetDisplay` returns
  # EGL_NO_DISPLAY, WebKit prints `Could not create default EGL display:
  # EGL_BAD_PARAMETER. Aborting...`, and the WebProcess crashes — leaving
  # the tauri window as a blank whitescreen. Point libglvnd at nixpkgs'
  # mesa (which ships `libEGL_mesa.so.0`, `swrast_dri.so` and the matching
  # `egl_vendor.d` JSON) and force software rendering so we don't try to
  # talk to the host's NVIDIA/Mesa drivers (whose ABIs would clash with
  # the nix-built lib stack).
  WEBKIT_DISABLE_DMABUF_RENDERER = "1";
  LIBGL_ALWAYS_SOFTWARE = "1";
  __EGL_VENDOR_LIBRARY_DIRS = "${pkgs.mesa}/share/glvnd/egl_vendor.d";
  LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";

  shellHook = ''
    # cc-wrapper's add-flags.sh prepends `-rpath $out/lib` to NIX_LDFLAGS so
    # mkDerivation builds get a working RUNPATH at install time. In a
    # nix-shell, however, $out resolves to <repo>/outputs/out — a path that
    # never exists — so every binary cargo links here (build scripts and
    # the tauri app itself) ends up with a dead RUNPATH and can't load its
    # dynamic deps at runtime. Rewrite the bogus rpath to point at the
    # link-time inputs so RUNPATH actually resolves and we don't have to
    # leak everything onto LD_LIBRARY_PATH.
    if [ -n "$out" ] && [[ "$NIX_LDFLAGS" == *"-rpath $out/lib"* ]]; then
      _rpath_real=${pkgs.lib.makeLibraryPath tauriLinkLibs}
      export NIX_LDFLAGS="''${NIX_LDFLAGS//-rpath $out\/lib/-rpath $_rpath_real}"
      unset _rpath_real
    fi

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
