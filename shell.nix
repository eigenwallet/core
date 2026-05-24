{ pkgs ? import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/refs/heads/nixos-25.05.tar.gz";
    # allowUnfree lets nixGL build the proprietary NVIDIA user-space driver for
    # the GPU path below. Harmless on hosts without NVIDIA: nothing unfree is
    # pulled in unless that path is actually taken.
  }) { config.allowUnfree = true; }
, # NVIDIA user-space driver version for GPU rendering in the Tauri webview.
  # Defaults to auto-detecting the running kernel module from /proc, so it
  # tracks the host across driver upgrades instead of pinning one version.
  # Detection is impure (reads /proc, rebuilds each eval) which is fine under
  # `nix-shell`; pure callers like the flake's `nix develop` must pass this
  # explicitly (e.g. `nvidiaVersion = "580.159.03"`) or `null` to skip the GPU
  # path and use mesa software rendering.
  nvidiaVersion ?
    let
      # nix can't `readFile` /proc directly (zero-sized files, NixOS/nix#3539),
      # so copy it out in an impure derivation first — same trick as nixGL's
      # own `auto`. `|| touch` yields an empty file on non-NVIDIA hosts.
      versionFile = pkgs.runCommand "impure-nvidia-version" {
        time = builtins.currentTime; # rebuild every eval; the host driver can change
        preferLocalBuild = true;
        allowSubstitutes = false;
      } ''cp /proc/driver/nvidia/version "$out" 2>/dev/null || touch "$out"'';
      firstLine = builtins.head (pkgs.lib.splitString "\n" (builtins.readFile versionFile));
      # Match both "...Kernel Module  <ver>  ..." (proprietary) and
      # "...Open Kernel Module for x86_64  <ver>  ..." (open module). nixGL's
      # built-in detector only handles the former, so we parse it ourselves.
      m = builtins.match ".*Kernel Module( for [^ ]+)?  ([0-9.]+)  .*" firstLine;
    in if m == null then null else builtins.elemAt m 1
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

  # GPU rendering for the Tauri webview. WebKitGTK dispatches OpenGL/EGL through
  # nix's libglvnd, which ships no vendor ICD, so on a non-NixOS host the
  # WebProcess can't reach the GPU and either aborts with `EGL_BAD_PARAMETER`
  # or silently drops to CPU rasterisation (single-digit fps even on a discrete
  # GPU).
  #
  # nixGL fixes this by building the NVIDIA user-space driver *as a nix
  # derivation* and exposing it via LD_LIBRARY_PATH + a glvnd vendor ICD. The
  # crucial property — versus copying the host's driver libs, e.g. nix-gl-host
  # — is that the nix-built driver links nix's own libX11/libxcb/libffi, so it
  # never drags mismatched host libraries into the nix process. Pulling host
  # libxcb/libffi in alongside nix's copies crashes the WebProcess in their
  # `_init`, which is exactly what a host-driver bridge does here.
  #
  # The driver must match the running kernel module *exactly* (a mismatch falls
  # back to software or fails), which is why `nvidiaVersion` auto-detects rather
  # than pins. This is built only when a driver is present: the shellHook
  # references it solely inside the `nvidiaVersion != null` branch, and nix is
  # lazy, so a host without NVIDIA never fetches nixGL or the driver runfile.
  nixGLNvidia = (import (builtins.fetchTarball {
    url = "https://github.com/nix-community/nixGL/archive/refs/heads/main.tar.gz";
  }) { inherit pkgs nvidiaVersion; }).nixGLNvidia;

  # The GL environment, chosen at eval time so `just tauri-mainnet` runs
  # unwrapped. NVIDIA: source the nixGL wrapper's env-setup — strip its trailing
  # `exec "$@"` so sourcing doesn't exec an empty argv; the bash NVIDIA_JSON*
  # arrays it defines are evaluated too. The wrapper *appends* to the existing
  # LD_LIBRARY_PATH (the link-time libs set on the shell), so both survive.
  # GDK_BACKEND=x11 is forced because NVIDIA + native Wayland + nix's webkitgtk
  # hits a Wayland EPROTO during DMA-BUF setup; XWayland renders fine via the
  # GLX/EGL paths the wrapper wires up.
  gpuShellHook =
    if nvidiaVersion != null then ''
      eval "$(${pkgs.gnused}/bin/sed '/^[[:space:]]*exec /d' ${nixGLNvidia}/bin/nixGLNvidia-${nvidiaVersion})"
      export GDK_BACKEND=x11

      # Under GNOME fractional scaling + XWayland, app-set cursors (the hand
      # over a button, the I-beam over a text box) render tiny because GTK
      # ignores the X server's scaled Xcursor.size. Re-export that size as
      # XCURSOR_SIZE so they match the correctly-sized default cursor. No-op
      # when there's no X server or the resource is absent (non-HiDPI).
      if [ -z "''${XCURSOR_SIZE:-}" ] && [ -n "''${DISPLAY:-}" ]; then
        _xcursor_size=$(${pkgs.xorg.xrdb}/bin/xrdb -query 2>/dev/null | ${pkgs.gnused}/bin/sed -n 's/^Xcursor\.size:[[:space:]]*//p')
        [ -n "$_xcursor_size" ] && export XCURSOR_SIZE="$_xcursor_size"
        unset _xcursor_size
      fi
    '' else ''
      # No NVIDIA driver detected: fall back to nixpkgs' mesa software
      # rasteriser. Slow but portable; WEBKIT_DISABLE_DMABUF_RENDERER=1 stops
      # the WebProcess from attempting a hardware path it can't satisfy.
      export __EGL_VENDOR_LIBRARY_DIRS=${pkgs.mesa}/share/glvnd/egl_vendor.d
      export LIBGL_DRIVERS_PATH=${pkgs.mesa}/lib/dri
      export LIBGL_ALWAYS_SOFTWARE=1
      export WEBKIT_DISABLE_DMABUF_RENDERER=1
    '';

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

    # GPU rendering setup (see the nixGL / nvidiaVersion bindings above): NVIDIA
    # via a nix-built driver, otherwise mesa software rendering.
    ${gpuShellHook}
    # `just docker_test` runs the testcontainers-based integration tests, whose
    # 0.15 Cli client shells out to a `docker` binary. On a host with rootless
    # podman but no docker (e.g. Fedora), bridge `docker` -> `podman` so the
    # tests run without a docker daemon or root. Guarded so a real docker
    # install — or CI's preinstalled docker — is left untouched.
    if ! command -v docker >/dev/null 2>&1 && command -v podman >/dev/null 2>&1; then
      _docker_shim="$HOME/.cache/eigenwallet-docker-shim"
      mkdir -p "$_docker_shim"
      printf '#!/bin/sh\nexec podman "$@"\n' > "$_docker_shim/docker"
      chmod +x "$_docker_shim/docker"
      export PATH="$_docker_shim:$PATH"
      # testcontainers 0.15's Cli client cleans up its own containers; Ryuk
      # (its reaper container) isn't needed and trips on rootless podman.
      export TESTCONTAINERS_RYUK_DISABLED=true
      unset _docker_shim
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
