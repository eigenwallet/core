fn main() {
    #[cfg(target_os = "windows")]
    {
        // make sure the .dll's are exist -- else panic
        if !["libstdc++-6.dll", "libgcc_s_seh-1.dll"]
            .into_iter()
            .all(|dll| std::path::Path::new(dll).try_exists().unwrap_or(false))
        {
            panic!("libstdc++-6.dll and libgcc_s_seh-1.dll don't exist in the src-tauri directory - run `just prepare-windows-build` to build them");
        }

        let home_dir = std::env::var("HOME").expect("HOME environment variable not set");
        let lib_path = format!("{}/opt/gcc-mingw-14.3/x86_64-w64-mingw32/lib", home_dir);
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!("cargo:rustc-link-lib=stdc++");
    }

    tauri_build::build();
}
