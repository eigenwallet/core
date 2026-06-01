# Nix dev environment

A `nix-shell` / `nix develop` environment providing the full toolchain to build and
run eigenwallet (monero-sys, the Tauri GUI, and the `just` recipes) on non-NixOS hosts.
This file documents the *why* behind `shell.nix` and `flake.nix`; the code itself is
kept comment-free.

## Usage

- `nix-shell` — impure shell; auto-detects the host NVIDIA driver for GPU rendering.
- `direnv` — `.envrc` runs `use nix`, so the shell loads automatically on `cd`.
- `nix develop` — pure flake shell; uses mesa software rendering (pure eval can't read
  `/proc`). For GPU acceleration use `nix-shell`, or `nix develop --impure` with an
  explicit `nvidiaVersion` (e.g. `"580.159.03"`).

Inputs are pinned: `flake.lock` for the flake path, and an explicit rev + `sha256` on
each `fetchTarball` in `shell.nix` for the `nix-shell` path. Bump both together.

## GPU rendering (Tauri webview)

WebKitGTK dispatches OpenGL/EGL through nix's libglvnd, which ships no vendor ICD. On a
non-NixOS host the WebProcess can't reach the GPU and either aborts with
`EGL_BAD_PARAMETER` or silently drops to CPU rasterisation (single-digit fps even on a
discrete GPU).

We use [nixGL](https://github.com/nix-community/nixGL) to build the NVIDIA user-space
driver *as a nix derivation* and expose it via `LD_LIBRARY_PATH` + a glvnd vendor ICD.
Building it — rather than bridging the host's driver libs (e.g. nix-gl-host) — is what
keeps it working: the nix-built driver links nix's own libX11/libxcb/libffi, so it never
drags mismatched host libraries into the nix process. Pulling host libxcb/libffi in
alongside nix's copies crashes the WebProcess in their `_init`.

The driver must match the running kernel module *exactly*, so `nvidiaVersion`
auto-detects from `/proc/driver/nvidia/version` rather than pinning a version (`nix` can't
`readFile` `/proc` directly — zero-sized files, NixOS/nix#3539 — so it's copied out in an
impure derivation first; `builtins.currentTime` makes it re-detect each eval). The regex
matches both the proprietary and the open kernel module. nixGL is only fetched/built when
a driver is present — the shellHook references it solely in the `nvidiaVersion != null`
branch, and nix is lazy — so a host without NVIDIA never fetches it.

`GDK_BACKEND=x11` is forced because NVIDIA + native Wayland + nix's webkitgtk hits a
Wayland `EPROTO` during DMA-BUF setup; XWayland renders fine via the GLX/EGL paths nixGL
wires up. The wrapper's env-setup is sourced (with its trailing `exec` stripped so
sourcing doesn't exec an empty argv). Without a driver we fall back to mesa software
rendering and `WEBKIT_DISABLE_DMABUF_RENDERER=1`.

## monero-depends toolchain wrappers

`monero-depends/hosts/linux.mk` hardcodes the Debian-style triple `x86_64-linux-gnu-<tool>`
on x86 build hosts. Nixpkgs ships those tools unprefixed (or under
`x86_64-unknown-linux-gnu-*`), so without the `prefixWrapper` shims `configure` reports
"C compiler cannot create executables" even though the toolchain is fine.

## Linking model

Link-time deps (`tauriLinkLibs`) are found via pkg-config and the RPATH baked in by
`NIX_LDFLAGS`. We deliberately keep them off `LD_LIBRARY_PATH` so they can't shadow
nix-built tools like curl, whose ngtcp2 module is pinned to a specific openssl ABI.
`stdenv.cc.cc.lib` is included so RUNPATH covers `libstdc++.so.6`.

Only the subset WebKitGTK/GTK `dlopen` at runtime (`tauriRuntimeLibs`) goes on
`LD_LIBRARY_PATH`, because `dlopen` of a bare soname consults `LD_LIBRARY_PATH` and the
system cache, not the calling binary's RUNPATH.

In a `nix-shell`, cc-wrapper sets `-rpath $out/lib` in `NIX_LDFLAGS`, but `$out` resolves
to `<repo>/outputs/out` — a path that never exists — so every binary cargo links gets a
dead RUNPATH. The shellHook rewrites that rpath to the real link-time inputs.

## docker → podman shim

`just docker_test` runs testcontainers-based integration tests whose 0.15 Cli client
shells out to a `docker` binary. On a host with rootless podman but no docker (e.g.
Fedora), the shellHook bridges `docker` → `podman` (appended to `PATH`, so a real docker
always wins) and sets `TESTCONTAINERS_RYUK_DISABLED=true` — the reaper isn't needed and
trips on rootless podman. Guarded so a real docker install is left untouched.

## Rust & yarn

The Rust toolchain comes from host rustup (`~/.cargo/bin`, re-prepended to `PATH`);
`rust-toolchain.toml` pins the version. `src-gui` pins `yarn@4.x` via `packageManager`, so
corepack (bundled with nodejs) materialises that exact version into a user-writable dir
instead of using nixpkgs' yarn 1.x.

## electrs test image

`swap/tests/harness` pulls `vulpemventures/electrs:latest`: Docker Hub dropped the old
`v0.16.0.3` tag (it now 404s), and `latest` is still the same 2020 build with the same
`/build/electrs` entrypoint. The live test path already used `latest`.
