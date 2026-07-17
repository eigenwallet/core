# Vendored: tauri-plugin-webdriver 0.2.1

- Source: crates.io `tauri-plugin-webdriver` 0.2.1 (MIT)
- Upstream: https://github.com/Choochmeque/tauri-plugin-webdriver @ 6ffcf76ddcbca65eb58417af88054cad3c527ae4
- crates.io sha256: a130e5cc896b52a87d618b53e1ba025954af7c004b8781a4ddee7298f7b3749a
- Copied verbatim from the cargo registry cache (`~/.cargo/registry/src/*/tauri-plugin-webdriver-0.2.1/`), excluding registry metadata (`Cargo.toml.orig`, `Cargo.lock`, `.cargo_vcs_info.json`, `.cargo-ok`, `.github`, `.gitignore`, `.mlc.toml`). No source modifications.

To update: fetch the new version with cargo, diff against this tree, review, then re-copy.

This plugin embeds a W3C WebDriver server (default `127.0.0.1:4445`) that can execute arbitrary JavaScript in the app webview. It is only compiled with `--features automation` and must never be enabled in release builds.
