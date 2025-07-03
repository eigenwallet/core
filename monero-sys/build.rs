use cmake::Config;

fn main() {
    // On windows we use vcpkg to build the monero dependencies -- not on macos or linux because
    // its not absolutely necessary and takes a long time to build
    #[cfg(target_os = "windows")]
    {
        println!("cargo:debug=Building Monero dependencies with vcpkg");

        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        // Get the current PATH and remove msys64 entries to avoid cmake conflicts
        let current_path = std::env::var("PATH").unwrap_or_default();
        let filtered_path = current_path
            .split(';')
            .filter(|p| !p.to_lowercase().contains("msys64"))
            .collect::<Vec<_>>()
            .join(";");

        // Set the custom triplet path
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let triplet_path = format!("{}/x64-windows-static-md.cmake", manifest_dir);
        
        // Build dependencies, stream output to the console
        let mut child = Command::new("cargo-vcpkg")
            .args(["--verbose", "build"])
            .env("PATH", filtered_path)
            .env("VCPKG_DEFAULT_TRIPLET", "x64-windows-static-md")
            .env("VCPKG_OVERLAY_TRIPLETS", &manifest_dir)
            .env(
                "VCPKG_OVERLAY_PORTS",
                
// starts at core/target/vcpkg. only use windows notation since this runs only on windows
 "../../monero-sys/vendor/vcpkg-overlays/unbound;../../monero-sys/vendor/vcpkg-overlays/winflexbison",
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn vcpkg build process");

        // Stream stdout
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(line) = line {
                    println!("cargo:debug={}", line);
                }
            }
        }

        // Stream stderr
        if let Some(stderr) = child.stderr.take() {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    println!("cargo:debug={}", line);
                }
            }
        }

        let status = child.wait().expect("Failed to wait for vcpkg process");
        if !status.success() {
            panic!("vcpkg build failed with status: {}", status);
        }

        println!("cargo:debug=Finding vcpkg dependencies");

        // Set environment variable to indicate we want static libraries
        std::env::set_var("VCPKGRS_DYNAMIC", "0");

        // Configure vcpkg to use the correct triplet and root
        let mut config = vcpkg::Config::new();
        config.target_triplet("x64-windows-static-md");
        config.cargo_metadata(true);
        config.copy_dlls(false); // We want static libraries
        
        config.find_package("zeromq").unwrap();
        config.find_package("openssl").unwrap();
        config.find_package("boost").unwrap();
        config.find_package("libusb").unwrap();
        config.find_package("libsodium").unwrap();
        config.find_package("protobuf-c").unwrap();
        config.find_package("expat").unwrap();
        config.find_package("unbound").unwrap();
    }

    println!("cargo:warn=Building Monero");

    let is_github_actions: bool = std::env::var("GITHUB_ACTIONS").is_ok();

    // Only rerun this when the bridge.rs or static_bridge.h file changes.
    println!("cargo:rerun-if-changed=src/bridge.rs");
    println!("cargo:rerun-if-changed=src/bridge.h");

    // Build with the monero library all dependencies required
    let mut config = Config::new("monero");
    
    // On Windows, configure CMake to use vcpkg toolchain
    #[cfg(target_os = "windows")]
    {
        let vcpkg_root = std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/../target/vcpkg";
        let toolchain_file = format!("{}/scripts/buildsystems/vcpkg.cmake", vcpkg_root);
        let vcpkg_installed = format!("{}/installed/x64-windows-static-md", vcpkg_root);
        
        config.define("CMAKE_TOOLCHAIN_FILE", toolchain_file);
        config.define("VCPKG_TARGET_TRIPLET", "x64-windows-static-md");
        
        // Explicitly set OpenSSL paths for CMake
        config.define("OPENSSL_ROOT_DIR", &vcpkg_installed);
        config.define("OPENSSL_INCLUDE_DIR", format!("{}/include", &vcpkg_installed));
        config.define("OPENSSL_CRYPTO_LIBRARY", format!("{}/lib/libcrypto.lib", &vcpkg_installed));
        config.define("OPENSSL_SSL_LIBRARY", format!("{}/lib/libssl.lib", &vcpkg_installed));
        
        // Explicitly set unbound paths for CMake
        config.define("UNBOUND_ROOT_DIR", &vcpkg_installed);
        config.define("UNBOUND_INCLUDE_DIR", format!("{}/include", &vcpkg_installed));
        config.define("UNBOUND_LIBRARY", format!("{}/lib/unbound.lib", &vcpkg_installed));
        config.define("UNBOUND_LIBRARIES", format!("{}/lib/unbound.lib", &vcpkg_installed));
        
        // Explicitly set ZeroMQ paths for CMake
        config.define("ZMQ_ROOT", &vcpkg_installed);
        config.define("ZMQ_INCLUDE_PATH", format!("{}/include", &vcpkg_installed));
        config.define("ZMQ_LIB", format!("{}/lib/libzmq-mt-4_3_5.lib", &vcpkg_installed));
        
        // Explicitly set libsodium paths for CMake
        config.define("SODIUM_LIBRARY", format!("{}/lib/sodium.lib", &vcpkg_installed));
        config.define("SODIUM_INCLUDE_PATH", format!("{}/include", &vcpkg_installed));
        
        // Explicitly set Boost paths for CMake - vcpkg uses different paths than FindBoost
        config.define("BOOST_ROOT", &vcpkg_installed);
        config.define("Boost_INCLUDE_DIR", format!("{}/include", &vcpkg_installed));
        config.define("Boost_LIBRARY_DIR", format!("{}/lib", &vcpkg_installed));
        config.define("Boost_USE_STATIC_LIBS", "ON");
        config.define("Boost_USE_STATIC_RUNTIME", "OFF"); // We're using dynamic CRT
        config.define("Boost_USE_MULTITHREADED", "ON");
        config.define("Boost_NO_SYSTEM_PATHS", "ON");
        
        // Add required Boost components
        config.define("Boost_THREAD_LIBRARY", format!("{}/lib/boost_thread.lib", &vcpkg_installed));
        
        // Let CMake find the Boost libraries automatically
        // We've already set BOOST_ROOT and other paths above

        // Ensure Boost headers see we're linking statically and disable auto-linking
        config.define(
            "CMAKE_CXX_FLAGS",
            "/DBOOST_ALL_NO_LIB /DBOOST_ALL_STATIC_LINK /DBOOST_THREAD_USE_LIB /DBOOST_SERIALIZATION_STATIC_LINK",
        );
        
        // Force static runtime for all builds
        config.define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreadedDLL");
    }
    
    let output_directory = config
        .build_target("wallet_api")
        // Builds currently fail in Release mode
        // .define("CMAKE_BUILD_TYPE", "Release")
        // .define("CMAKE_RELEASE_TYPE", "Release")
        // Force building static libraries
        .define("STATIC", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("BUILD_TESTS", "OFF")
        .define("Boost_USE_STATIC_LIBS", "ON")
        .define("Boost_USE_STATIC_RUNTIME", "ON")
        //// Disable support for ALL hardware wallets
        // Disable Trezor support completely
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
        // Use lightweight crypto library
        .define("MONERO_WALLET_CRYPTO_LIBRARY", "cn");
        
    // Don't pass Make/Ninja specific flags to MSBuild on Windows
    #[cfg(not(target_os = "windows"))]
    let output_directory = config.build_arg(match is_github_actions {
        true => "-j1",
        false => "-j",
    }).build();
    
    #[cfg(target_os = "windows")]
    let output_directory = config.build();

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
    println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");

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

    // On macos we use homebrew to install the monero dependencies
    #[cfg(target_os = "macos")]
    {
        // Dynamically detect Homebrew installation prefix (works on both Apple Silicon and Intel Macs)
        let brew_prefix = std::process::Command::new("brew")
            .arg("--prefix")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "/opt/homebrew".into());

        // add homebrew search paths using dynamic prefix
        println!("cargo:rustc-link-search=native={}/lib", brew_prefix);
        println!(
            "cargo:rustc-link-search=native={}/opt/unbound/lib",
            brew_prefix
        );
        println!(
            "cargo:rustc-link-search=native={}/opt/expat/lib",
            brew_prefix
        );
        println!(
            "cargo:rustc-link-search=native={}/Cellar/protobuf@21/21.12_1/lib/",
            brew_prefix
        );

        // Add search paths for clang runtime libraries
        println !("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/clang/15.0.0/lib/darwin");
        println !("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/clang/16.0.0/lib/darwin");
        println !("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/clang/17.0.0/lib/darwin");
        println !("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/clang/18.0.0/lib/darwin");
    }

    // On linux we use apt to install the monero dependencies, they are found automatically

    // Link libwallet and libwallet_api statically
    println!("cargo:rustc-link-lib=static=wallet");
    println!("cargo:rustc-link-lib=static=wallet_api");

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

    // Static linking for boost (vcpkg naming convention)
    #[cfg(target_os = "windows")]
    {
        use std::fs;
        use std::path::Path;
        
        // Find the actual boost libraries installed by vcpkg
        let vcpkg_root = std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/../target/vcpkg";
        let boost_lib_dir = format!("{}/installed/x64-windows-static-md/lib", vcpkg_root);
        
        // Helper function to find boost library
        let find_boost_lib = |prefix: &str| -> Option<String> {
            if let Ok(entries) = fs::read_dir(&boost_lib_dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.starts_with(&format!("boost_{}-", prefix)) && name.ends_with(".lib") && !name.contains("-gd-") {
                            return Some(name.trim_end_matches(".lib").to_string());
                        }
                    }
                }
            }
            None
        };
        
        // Link the boost libraries we find
        for lib in &["serialization", "filesystem", "thread", "chrono", "system", "date_time", "program_options", "locale"] {
            if let Some(lib_name) = find_boost_lib(lib) {
                println!("cargo:rustc-link-lib=static={}", lib_name);
            } else {
                // Fallback to generic name
                println!("cargo:rustc-link-lib=static=boost_{}", lib);
            }
        }
    }
    
    // Static linking for boost (standard naming on non-Windows)
    #[cfg(not(target_os = "windows"))]
    {
        println!("cargo:rustc-link-lib=static=boost_serialization");
        println!("cargo:rustc-link-lib=static=boost_filesystem");
        println!("cargo:rustc-link-lib=static=boost_thread");
        println!("cargo:rustc-link-lib=static=boost_chrono");
        println!("cargo:rustc-link-lib=static=boost_system");
        println!("cargo:rustc-link-lib=static=boost_date_time");
        println!("cargo:rustc-link-lib=static=boost_locale");
        println!("cargo:rustc-link-lib=static=boost_program_options");
    }

    // Link libsodium statically
    println!("cargo:rustc-link-lib=static=sodium");

    // Link OpenSSL statically
    println!("cargo:rustc-link-lib=static=ssl"); // This is OpenSSL (libsll)
    println!("cargo:rustc-link-lib=static=crypto"); // This is OpenSSLs crypto library (libcrypto)

    // Link unbound statically
    println!("cargo:rustc-link-lib=static=unbound");
    println!("cargo:rustc-link-lib=static=expat"); // Expat is required by unbound
    println!("cargo:rustc-link-lib=static=nghttp2");
    println!("cargo:rustc-link-lib=static=event");

    // Link protobuf statically
    println!("cargo:rustc-link-lib=static=protobuf");

    #[cfg(target_os = "macos")]
    {
        // Static archive is always present, dylib only on some versions.
        println!("cargo:rustc-link-lib=static=clang_rt.osx");

        // Minimum OS version you already add:
        println!("cargo:rustc-link-arg=-mmacosx-version-min=11.0");
    }

    // Build the CXX bridge
    let mut build = cxx_build::bridge("src/bridge.rs");

    #[cfg(target_os = "macos")]
    {
        build.flag_if_supported("-mmacosx-version-min=11.0");
    }

    build
        .flag_if_supported("-std=c++17")
        .include("src") // Include the bridge.h file
        .include("monero/src") // Includes the monero headers
        .include("monero/external/easylogging++") // Includes the easylogging++ headers
        .include("monero/contrib/epee/include"); // Includes the epee headers for net/http_client.h
        
    // Add platform-specific include paths
    #[cfg(target_os = "windows")]
    {
        let vcpkg_root = std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/../target/vcpkg";
        let vcpkg_installed = format!("{}/installed/x64-windows-static-md", vcpkg_root);
        build.include(format!("{}/include", vcpkg_installed));
    }
    
    #[cfg(target_os = "macos")]
    {
        build.include("/opt/homebrew/include"); // Homebrew include path for Boost
    }
    
    // Position independent code (not needed on Windows)
    #[cfg(not(target_os = "windows"))]
    build.flag("-fPIC");
        
    // Windows-specific flags for static linking
    #[cfg(target_os = "windows")]
    {
        build
            .flag("/DBOOST_ALL_NO_LIB")
            .flag("/DBOOST_ALL_STATIC_LINK")
            .flag("/DBOOST_THREAD_USE_LIB")
            .flag("/DBOOST_THREAD_BUILD_LIB")
            .flag("/DBOOST_SERIALIZATION_STATIC_LINK")
            .flag("/DBOOST_SYSTEM_STATIC_LINK")
            .flag("/DBOOST_FILESYSTEM_STATIC_LINK")
            .flag("/DBOOST_CHRONO_STATIC_LINK")
            .flag("/DBOOST_DATE_TIME_STATIC_LINK")
            .flag("/DBOOST_LOCALE_STATIC_LINK")
            .flag("/DBOOST_PROGRAM_OPTIONS_STATIC_LINK")
            .flag("/D_WIN32_WINNT=0x0601")  // Windows 7 minimum
            .flag("/DWIN32_LEAN_AND_MEAN");
    }
    
    // Non-Windows flags
    #[cfg(not(target_os = "windows"))]
    {
        build
            .flag("-DBOOST_ALL_NO_LIB")
            .flag("-DBOOST_ARCHIVE_STATIC_LINK")
            .flag("-DBOOST_SERIALIZATION_STATIC_LINK")
            .flag("-DBOOST_USE_STATIC_LIBS");
    }

    #[cfg(target_os = "macos")]
    {
        // Use the same dynamic brew prefix for include paths
        let brew_prefix = std::process::Command::new("brew")
            .arg("--prefix")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "/opt/homebrew".into());

        build.include(format!("{}/include", brew_prefix)); // Homebrew include path for Boost
    }

    build.compile("monero-sys");
}
