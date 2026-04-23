use std::path::Path;

/// Verify that the updater still uses a reqwest build with `socks` enabled.
///
/// `reqwest13` is a compatibility dependency for `tauri-plugin-updater`.
/// Update this check if the updater moves to another reqwest version.
fn reqwest_dep_version(dep: &toml::Value) -> Option<&str> {
    dep.as_table()?.get("version")?.as_str()
}

fn reqwest_dep_package(dep: &toml::Value) -> Option<&str> {
    dep.as_table()?.get("package")?.as_str()
}

fn reqwest_dep_has_feature(dep: &toml::Value, feature: &str) -> bool {
    dep.as_table()
        .and_then(|table| table.get("features"))
        .and_then(toml::Value::as_array)
        .is_some_and(|features| features.iter().any(|value| value.as_str() == Some(feature)))
}

fn verify_reqwest13_manifest(manifest_path: &Path) {
    let manifest =
        std::fs::read_to_string(manifest_path).expect("cannot read src-tauri/Cargo.toml");
    let manifest: toml::Value =
        toml::from_str(&manifest).expect("src-tauri/Cargo.toml must be valid TOML");

    let reqwest13 = manifest
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("reqwest13"))
        .expect("`reqwest13` dependency missing from src-tauri/Cargo.toml");

    let Some(package) = reqwest_dep_package(reqwest13) else {
        panic!("`reqwest13` must stay an explicit dependency table with `package = \"reqwest\"`");
    };
    if package != "reqwest" {
        panic!("`reqwest13` must continue aliasing the `reqwest` crate");
    }

    let Some(version) = reqwest_dep_version(reqwest13) else {
        panic!("`reqwest13` must declare an explicit reqwest version");
    };
    if !version.starts_with("0.13") {
        panic!("`reqwest13` must continue targeting reqwest 0.13.x until updater changes");
    }

    if !reqwest_dep_has_feature(reqwest13, "socks") {
        panic!(
            "`reqwest13` must keep `features = [\"socks\"]` or updater SOCKS5 support regresses"
        );
    }
}

fn verify_updater_reqwest_in_lock(lock_path: &Path) {
    let lock = std::fs::read_to_string(lock_path).expect("cannot read Cargo.lock");
    let lock: toml::Value = toml::from_str(&lock).expect("Cargo.lock must be valid TOML");

    let updater = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .and_then(|packages| {
            packages.iter().find(|package| {
                package
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|name| name == "tauri-plugin-updater")
            })
        })
        .expect("tauri-plugin-updater not found in Cargo.lock");

    let dependencies = updater
        .get("dependencies")
        .and_then(toml::Value::as_array)
        .expect("tauri-plugin-updater has no dependencies in Cargo.lock — SOCKS5 proxy support needs review");

    for dependency in dependencies {
        let Some(dependency) = dependency.as_str() else {
            continue;
        };
        let mut parts = dependency.split_whitespace();
        if parts.next() != Some("reqwest") {
            continue;
        }

        let Some(version) = parts.next() else {
            panic!("tauri-plugin-updater reqwest dependency in Cargo.lock has unexpected format");
        };
        if !version.starts_with("0.13.") {
            panic!(
                "\n\n\
                !! SOCKS5 proxy regression !!\n\
                tauri-plugin-updater now uses reqwest {version}, but the\n\
                `reqwest13` phantom dependency in src-tauri/Cargo.toml targets 0.13.\n\
                Update the phantom dependency version to match, then verify\n\
                that the new reqwest still has a `socks` feature.\n\n"
            );
        }
        return;
    }

    panic!(
        "tauri-plugin-updater no longer depends on reqwest — \
         SOCKS5 proxy support needs review"
    );
}

fn verify_updater_reqwest_socks() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    let lock_path = Path::new(&manifest_dir)
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");

    println!("cargo:rerun-if-changed={}", manifest_path.display());
    println!("cargo:rerun-if-changed={}", lock_path.display());

    verify_reqwest13_manifest(&manifest_path);
    verify_updater_reqwest_in_lock(&lock_path);
}

fn main() {
    verify_updater_reqwest_socks();
    #[cfg(target_os = "windows")]
    {
        #[cfg(not(host_os = "linux"))]
        {
            panic!("Compiling for Windows is currently only supported from Linux (x86_64)");
        }

        // make sure the .dll's are exist -- else panic
        if !["libstdc++-6.dll", "libgcc_s_seh-1.dll"]
            .into_iter()
            .all(|dll| std::path::Path::new(dll).try_exists().unwrap_or(false))
        {
            panic!(
                "libstdc++-6.dll and libgcc_s_seh-1.dll don't exist in the src-tauri directory - run `just prepare-windows-build` to build them"
            );
        }

        let home_dir = std::env::var("HOME").expect("HOME environment variable not set");
        let lib_path = format!("{}/opt/gcc-mingw-14.3/x86_64-w64-mingw32/lib", home_dir);
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!("cargo:rustc-link-lib=stdc++");
    }

    tauri_build::build();
}
