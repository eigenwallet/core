use cmake::Config;
use fs_extra::error::ErrorKind;
use std::fs;
use std::io::Write as _;
use std::path::Path;

/// Directory at which this repository is stored as a submodule
/// https://github.com/eigenwallet/monero-depends
///
/// See `.gitmodules` at the root of workspace.
static MONERO_DEPENDS_DIR: &str = "monero-depends";

/// Directory at which the Monero C++ codebase is stored as a submodule
///
/// See `.gitmodules` at the root of workspace.
static MONERO_CPP_DIR: &str = "monero";

/// Represents a patch to be applied to the Monero codebase
struct EmbeddedPatch {
    name: &'static str,
    description: &'static str,
    patch_unified: &'static str,
}

/// Macro to create embedded patches with compile-time file inclusion
macro_rules! embedded_patch {
    ($name:literal, $description:literal, $patch_file:literal) => {
        EmbeddedPatch {
            name: $name,
            description: $description,
            patch_unified: include_str!($patch_file),
        }
    };
}

/// Embedded patches applied at compile time
const EMBEDDED_PATCHES: &[EmbeddedPatch] = &[
    embedded_patch!(
        "eigenwallet_0001_wallet2_api_allow_subtract_from_fee",
        "Adds subtract_fee_from_outputs parameter to wallet2_api transaction creation methods",
        "patches/eigenwallet_0001_wallet2_api_allow_subtract_from_fee.patch"
    ),
    embedded_patch!(
        "0001-fix-dummy-translation-generator.patch",
        "Creates dummy translation generator",
        "patches/0001-fix-dummy-translation-generator.patch"
    ),
    embedded_patch!(
        "0002-fix-iOS-depends-build.patch",
        "Fixes iOS depends build",
        "patches/0002-fix-iOS-depends-build.patch"
    ),
    embedded_patch!(
        "0003-include-locale-only-when-targeting-WIN32.patch",
        "Includes locale only when targeting WIN32 to fix cross-platform build issues",
        "patches/0003-include-locale-only-when-targeting-WIN32.patch"
    ),
    embedded_patch!(
        "0004-fix-___isPlatformVersionAtLeast.patch",
        "Fixes ___isPlatformVersionAtLeast being called",
        "patches/0004-fix-___isPlatformVersionAtLeast.patch"
    ),
    embedded_patch!(
        "0002-store-crash-fix",
        "Fixes corrupted wallet cache when storing while refreshing",
        "patches/0002-store-crash-fix.patch"
    ),
    embedded_patch!(
        "eigenwallet_0002_wallet2_increase_rpc_retries",
        "Increases the number of RPC retries for wallet2::refresh from 3 to 10",
        "patches/eigenwallet_0002_wallet2_increase_rpc_retries.patch"
    ),
    embedded_patch!(
        "eigenwallet_0003_pending_transaction_tx_keys",
        "Adds txKeys() to PendingTransaction in wallet2_api.h",
        "patches/eigenwallet_0003_pending_transaction_tx_keys.patch"
    ),
    embedded_patch!(
        "eigenwallet_0004_wallet_impl_balance_per_subaddress.patch",
        "Adds balancePerSubaddress() and unlockedBalancePerSubaddress() to wallet::WalletImpl in api/wallet.h",
        "patches/eigenwallet_0004_wallet_impl_balance_per_subaddress.patch"
    ),
];

/// Find the workspace target directory from OUT_DIR
///
/// OUT_DIR is something like: /path/to/workspace/target/debug/build/monero-sys-abc123/out
/// We want to extract: /path/to/workspace/target
fn find_workspace_target_dir() -> std::path::PathBuf {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR to be set");
    let out_path = Path::new(&out_dir);

    // Walk up from OUT_DIR to find "target" directory
    for ancestor in out_path.ancestors() {
        // allow target dir and also target-check dir (latter one is for lsp to not interfere with cli build commands)
        if ancestor.ends_with("target") || ancestor.ends_with("target-check") {
            return ancestor.to_path_buf();
        }
    }

    panic!("Could not find target directory from OUT_DIR: {out_dir}");
}

fn main() {
    let is_github_actions: bool = std::env::var("GITHUB_ACTIONS").is_ok();
    let is_docker_build: bool = std::env::var("DOCKER_BUILD").is_ok();

    // Rerun this when the bridge.rs or static_bridge.h file changes.
    println!("cargo:rerun-if-changed=src/bridge.rs");
    println!("cargo:rerun-if-changed=src/bridge.h");

    // Rerun if this build script changes (since it contains embedded patches)
    println!("cargo:rerun-if-changed=build.rs");

    // Rerun if the patches directory or any patch files change
    println!("cargo:rerun-if-changed=patches");

    // Apply embedded patches before building
    apply_patches().expect("Failed to apply our patches");

    // flush std::out
    std::io::stdout().flush().unwrap();
    std::io::stderr().flush().unwrap();

    let contrib_depends_dir = std::env::current_dir()
        .expect("current directory to be accessible")
        .join(MONERO_DEPENDS_DIR);

    // Use stable location in target/debug/monero-depends to avoid rebuilding deps unnecessarily
    let target_dir = find_workspace_target_dir();
    let stable_depends_dir = target_dir
        .join("debug")
        .join("monero-depends")
        .join(std::env::var("TARGET").expect("TARGET env var to be present"));

    let (contrib_depends_dir, target) =
        compile_dependencies(contrib_depends_dir, stable_depends_dir);

    // Build with the monero library all dependencies required
    let mut config = Config::new(MONERO_CPP_DIR);

    let toolchain_file = contrib_depends_dir
        .join(format!("{target}/share/toolchain.cmake"))
        .display()
        .to_string();
    config.define("CMAKE_TOOLCHAIN_FILE", toolchain_file.clone());
    println!("cargo:debug=Using toolchain file: {toolchain_file}");

    let depends_lib_dir = contrib_depends_dir.join(format!("{target}/lib"));

    println!(
        "cargo:rustc-link-search=native={}",
        depends_lib_dir.display()
    );

    let output_directory = config
        .build_target("wallet_api")
        // Builds currently fail in Release mode
        // .define("CMAKE_BUILD_TYPE", "Release")
        // .define("CMAKE_RELEASE_TYPE", "Release")
        // Force building static libraries
        .define("STATIC", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("BUILD_TESTS", "OFF")
        .define("USE_DEVICE_TREZOR", "OFF")
        .define("USE_DEVICE_TREZOR_MANDATORY", "OFF")
        .define("USE_DEVICE_TREZOR_PROTOBUF_TEST", "OFF")
        .define("USE_DEVICE_TREZOR_LIBUSB", "OFF")
        .define("USE_DEVICE_TREZOR_UDP_RELEASE", "OFF")
        .define("USE_DEVICE_TREZOR_DEBUG", "OFF")
        .define("TREZOR_DEBUG", "OFF")
        // Prevent CMake from finding dependencies that could enable Trezor
        .define("CMAKE_DISABLE_FIND_PACKAGE_LibUSB", "ON")
        // Disable Ledger support
        .define("USE_DEVICE_LEDGER", "OFF")
        .define("CMAKE_DISABLE_FIND_PACKAGE_HIDAPI", "ON")
        .define("GTEST_HAS_ABSL", "OFF")
        .define("SODIUM_LIBRARY", "libsodium.a")
        // Use lightweight crypto library
        .define("MONERO_WALLET_CRYPTO_LIBRARY", "cn")
        .define("CMAKE_CROSSCOMPILING", "OFF")
        .define(
            "SODIUM_INCLUDE_PATH",
            contrib_depends_dir
                .join(format!("{target}/include"))
                .display()
                .to_string(),
        ) // This is needed for libsodium.a to be found on mingw-w64
        .build_arg("-Wno-dev") // Disable warnings we can't fix anyway
        .build_arg(format!(
            "-j{}",
            if is_github_actions || is_docker_build {
                1
            } else {
                num_cpus::get()
            }
        ))
        .build_arg("-I.")
        .build();

    let monero_build_dir = output_directory.join("build");

    println!(
        "cargo:debug=Build directory: {}",
        output_directory.display()
    );

    // Add output directories to the link search path
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("lib").display()
    );

    // Add additional link search paths for libraries in different directories
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("contrib/epee/src").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("external/easylogging++").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir
            .join("external/db_drivers/liblmdb")
            .display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("external/randomx").display()
    );

    if target.contains("linux") && target.contains("x86_64") {
        println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");
    }
    if target.contains("linux") && target.contains("aarch64") {
        println!("cargo:rustc-link-search=native=/usr/lib/aarch64-linux-gnu");
    }

    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/crypto").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/net").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/ringct").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/checkpoints").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/multisig").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/cryptonote_basic").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/common").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/cryptonote_core").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/hardforks").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/blockchain_db").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/device").display()
    );
    // device_trezor search path (stub version when disabled)
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/device_trezor").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/mnemonics").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/rpc").display()
    );

    // Add search paths for clang runtime libraries on macOS (not iOS)
    if target.contains("apple-darwin") {
        // Dynamically detect Homebrew installation prefix (works on both Apple Silicon and Intel Macs)
        let brew_prefix = std::process::Command::new("brew")
            .arg("--prefix")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "/opt/homebrew".into());

        // add homebrew search paths using dynamic prefix
        println!("cargo:rustc-link-search=native={brew_prefix}/lib",);
        println!("cargo:rustc-link-search=native={brew_prefix}/opt/unbound/lib",);
        println!("cargo:rustc-link-search=native={brew_prefix}/opt/expat/lib",);
        println!("cargo:rustc-link-search=native={brew_prefix}/Cellar/protobuf@21/21.12_1/lib/",);

        // Add search paths for clang runtime libraries
        let resource_dir = std::process::Command::new("clang")
            .arg("-print-resource-dir")
            .output()
            .expect("clang")
            .stdout;
        let resource_dir = String::from_utf8_lossy(&resource_dir).trim().to_owned();
        println!("cargo:rustc-link-search=native={resource_dir}/lib/darwin");
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }

    // Link libwallet_api before libwallet for correct static link resolution on GNU ld
    println!("cargo:rustc-link-lib=static=wallet_api");
    println!("cargo:rustc-link-lib=static=wallet");

    // Link targets of monero codebase statically
    println!("cargo:rustc-link-lib=static=epee");
    println!("cargo:rustc-link-lib=static=easylogging");
    println!("cargo:rustc-link-lib=static=lmdb");
    println!("cargo:rustc-link-lib=static=randomx");
    println!("cargo:rustc-link-lib=static=cncrypto");
    println!("cargo:rustc-link-lib=static=net");
    println!("cargo:rustc-link-lib=static=ringct");
    println!("cargo:rustc-link-lib=static=ringct_basic");
    println!("cargo:rustc-link-lib=static=checkpoints");
    println!("cargo:rustc-link-lib=static=multisig");
    println!("cargo:rustc-link-lib=static=version");
    println!("cargo:rustc-link-lib=static=cryptonote_basic");
    println!("cargo:rustc-link-lib=static=cryptonote_format_utils_basic");
    println!("cargo:rustc-link-lib=static=common");
    println!("cargo:rustc-link-lib=static=cryptonote_core");
    println!("cargo:rustc-link-lib=static=hardforks");
    println!("cargo:rustc-link-lib=static=blockchain_db");
    println!("cargo:rustc-link-lib=static=device");
    // Link device_trezor (stub version when USE_DEVICE_TREZOR=OFF)
    println!("cargo:rustc-link-lib=static=device_trezor");
    println!("cargo:rustc-link-lib=static=mnemonics");
    println!("cargo:rustc-link-lib=static=rpc_base");

    // Static linking for boost
    println!("cargo:rustc-link-lib=static=boost_serialization");
    println!("cargo:rustc-link-lib=static=boost_filesystem");
    println!("cargo:rustc-link-lib=static=boost_thread");
    println!("cargo:rustc-link-lib=static=boost_chrono");
    println!("cargo:rustc-link-lib=static=boost_program_options");

    if target.contains("w64-mingw32") {
        println!("cargo:rustc-link-lib=static=boost_locale");
        println!("cargo:rustc-link-lib=static=iconv");

        // Link C++ standard library and GCC runtime statically
        println!("cargo:rustc-link-arg=-static-libstdc++");
        println!("cargo:rustc-link-arg=-static-libgcc");
    }

    // Link libsodium statically
    println!("cargo:rustc-link-lib=static=sodium");

    // Link OpenSSL statically (on android we use openssl-sys's vendored version instead)
    #[cfg(not(target_os = "android"))]
    {
        println!("cargo:rustc-link-lib=static=ssl"); // This is OpenSSL (libsll)
        println!("cargo:rustc-link-lib=static=crypto"); // This is OpenSSLs crypto library (libcrypto)
    }

    // Link unbound statically
    println!("cargo:rustc-link-lib=static=unbound");
    println!("cargo:rustc-link-lib=static=expat"); // Expat is required by unbound
                                                   // println!("cargo:rustc-link-lib=static=nghttp2");
                                                   // println!("cargo:rustc-link-lib=static=event");
                                                   // Android
    #[cfg(target_os = "android")]
    {
        println!("cargo:rustc-link-search=/home/me/Android/Sdk/ndk/27.3.13750724/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/");
        // println!("cargo:rustc-link-lib=static=c++_static");
    }

    // Link protobuf statically
    // println!("cargo:rustc-link-lib=static=protobuf");

    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-mmacosx-version-min=11.0");

    // Build the CXX bridge
    let mut build = cxx_build::bridge("src/bridge.rs");

    if target.contains("apple-ios") {
        // required for ___chkstk_darwin to be available
        build.flag_if_supported("-mios-version-min=13.0");
        println!("cargo:rustc-link-arg=-mios-version-min=13.0");
        println!("cargo:rustc-link-lib=framework=SystemConfiguration");
        println!("cargo:rustc-env=IPHONEOS_DEPLOYMENT_TARGET=13.0");
    }

    build
        .flag_if_supported("-std=c++17")
        .include("src") // Include the bridge.h file
        .include("monero/src") // Includes the monero headers
        .include("monero/external/easylogging++") // Includes the easylogging++ headers
        .include("monero/external")
        .include("monero/contrib/epee/include") // Includes the epee headers for net/http_client.h
        .include(
            contrib_depends_dir
                .join(format!("{target}/include"))
                .display()
                .to_string(),
        )
        .include(output_directory)
        .flag("-fPIC") // Position independent code
        .flag("-Wno-unused-parameter") // Suppress warnings from upstream Monero C++ headers
        .flag("-Wno-reorder-ctor"); // Suppress harmless ctor init order warning from wallet2.h

    build.compile("monero-sys");
}

/// Compile the dependencies
fn compile_dependencies(
    contrib_depends: std::path::PathBuf,
    out_dir: std::path::PathBuf,
) -> (std::path::PathBuf, String) {
    let mut target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    target = match target.as_str() {
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu".to_string(),
        "armv7-linux-androideabi" => "armv7a-linux-androideabi".to_string(),
        "x86_64-pc-windows-gnu" => "x86_64-w64-mingw32".to_string(),
        "aarch64-apple-ios-sim" => "aarch64-apple-iossimulator".to_string(),
        _ => target,
    };
    println!("cargo:debug=Building for target: {target}");

    match target.as_str() {
        "x86_64-apple-darwin"
        | "aarch64-apple-darwin"
        | "aarch64-apple-ios"
        | "aarch64-apple-iossimulator"
        | "x86_64-unknown-linux-gnu"
        | "aarch64-linux-gnu"
        | "aarch64-linux-android"
        | "x86_64-linux-android"
        | "armv7a-linux-androideabi"
        | "x86_64-w64-mingw32" => {}
        _ => panic!("target unsupported: {target}"),
    }

    println!("cargo:debug=Running make HOST={target} in contrib/depends",);

    // Copy monero-depends to out_dir/depends in order to build the dependencies there
    match fs_extra::copy_items(
        &[&contrib_depends],
        &out_dir,
        &fs_extra::dir::CopyOptions::new().copy_inside(true),
    ) {
        Ok(_) => (),
        Err(e) if matches!(e.kind, ErrorKind::AlreadyExists) => (), // Ignore the error if the directory already exists
        Err(e) => {
            eprintln!("Failed to copy contrib/depends to target dir: {e}");
            std::process::exit(1);
        }
    }

    let mut cmd = std::process::Command::new("env");
    if target.contains("-apple-") {
        cmd.arg("-i");
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.arg(format!("PATH={path}"));
    }
    cmd.arg("make")
        .arg(format!("HOST={target}"))
        .arg("DEBUG=")
        .current_dir(&out_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let child = cmd
        .spawn()
        .expect("[make depends] make command to be executable");

    let status = execute_child_with_pipe(child, String::from("[make depends] "))
        .expect("[make depends] make command to execute");

    if !status.success() {
        panic!(
            "[make depends] command failed with exit code: {:?}",
            status.code()
        );
    }

    println!("cargo:info=[make depends] make command completed successfully");

    (out_dir, target)
}

/// Execute a child process with piped stdout/stderr and display output in real-time
fn execute_child_with_pipe(
    mut child: std::process::Child,
    prefix: String,
) -> Result<std::process::ExitStatus, Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader};
    use std::thread;

    let stdout = child.stdout.take().expect("Failed to get stdout");
    let stderr = child.stderr.take().expect("Failed to get stderr");

    let prefix_clone = prefix.clone();
    // Spawn threads to handle stdout and stderr
    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            println!("cargo:debug={prefix}{line}");
        }
    });

    let stderr_handle = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            println!("cargo:debug={prefix_clone}{line}");
        }
    });

    // Wait for the process to complete
    let status = child.wait()?;

    // Wait for output threads to complete
    stdout_handle.join().unwrap();
    stderr_handle.join().unwrap();

    Ok(status)
}

/// Applies the [`EMBEDDED_PATCHES`] to the monero codebase.
fn apply_patches() -> Result<(), Box<dyn std::error::Error>> {
    let monero_dir = Path::new(MONERO_CPP_DIR);

    if !monero_dir.exists() {
        return Err("Monero directory not found. Please ensure the monero submodule is initialized and present.".into());
    }

    for embedded in EMBEDDED_PATCHES {
        println!(
            "cargo:debug=Processing embedded patch: {} ({})",
            embedded.name, embedded.description
        );

        // Split the patch into individual file patches
        let file_patches = split_patch_by_files(embedded.patch_unified)
            .map_err(|e| format!("Failed to split patch {}: {}", embedded.name, e))?;

        if file_patches.is_empty() {
            return Err(format!("No file patches found in patch {}", embedded.name).into());
        }

        println!(
            "cargo:debug=Found {} file(s) in patch {}",
            file_patches.len(),
            embedded.name
        );

        // Apply each file patch individually
        for (file_path, patch_content) in file_patches {
            println!("cargo:debug=Applying patch to file: {file_path}");

            // Parse the individual file patch
            let patch = diffy::Patch::from_str(&patch_content)
                .map_err(|e| format!("Failed to parse patch for {file_path}: {e}"))?;

            let target_path = monero_dir.join(&file_path);

            if !target_path.exists() {
                return Err(format!("Target file {file_path} not found!").into());
            }

            let current = fs::read_to_string(&target_path)
                .map_err(|e| format!("Failed to read {file_path}: {e}"))?;

            // Check if patch is already applied by trying to reverse it
            if diffy::apply(&current, &patch.reverse()).is_ok() {
                println!("cargo:debug=Patch for {file_path} already applied – skipping",);
                continue;
            }

            let patched = diffy::apply(&current, &patch)
                .map_err(|e| format!("Failed to apply patch to {file_path}: {e}"))?;

            fs::write(&target_path, patched)
                .map_err(|e| format!("Failed to write {file_path}: {e}"))?;

            println!("cargo:debug=Successfully applied patch to: {file_path}");
        }

        println!(
            "cargo:debug=Successfully applied all file patches for: {} ({})",
            embedded.name, embedded.description
        );
    }

    Ok(())
}

/// Split a multi-file patch into individual file patches
fn split_patch_by_files(
    patch_content: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut file_patches = Vec::new();
    let lines: Vec<&str> = patch_content.lines().collect();

    let mut current_file_patch = String::new();
    let mut current_file_path: Option<String> = None;
    let mut in_file_section = false;

    for line in lines {
        if line.starts_with("diff --git ") {
            // Save previous file patch if we have one
            if let Some(file_path) = current_file_path.take() {
                if !current_file_patch.trim().is_empty() {
                    file_patches.push((file_path, current_file_patch.clone()));
                }
            }

            // Start new file patch
            current_file_patch.clear();
            current_file_patch.push_str(line);
            current_file_patch.push('\n');

            // Extract file path from diff line (e.g., "diff --git a/src/wallet/api/wallet.cpp b/src/wallet/api/wallet.cpp")
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let file_path = parts[2].strip_prefix("a/").unwrap_or(parts[2]);
                current_file_path = Some(file_path.to_string());
            }
            in_file_section = true;
        } else if in_file_section {
            current_file_patch.push_str(line);
            current_file_patch.push('\n');
        }
    }

    // Don't forget the last file
    if let Some(file_path) = current_file_path {
        if !current_file_patch.trim().is_empty() {
            file_patches.push((file_path, current_file_patch));
        }
    }

    Ok(file_patches)
}
