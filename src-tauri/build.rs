fn main() {
    #[cfg(target_os = "windows")] {
        println!("cargo:rustc-link-search=native=/home/me/opt/gcc-mingw-14.3/x86_64-w64-mingw32/lib");
        println!("cargo:rustc-link-lib=stdc++");
    }
    tauri_build::build();
}
