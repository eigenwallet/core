use crate::compose::ASB_RPC_AUTH_FILE_ON_HOST;
use std::io::Write;

pub fn generate_rpc_auth_keyfile() {
    let password = dialoguer::Password::new()
        .with_prompt("Enter a strong RPC password")
        .with_confirmation("Confirm password", "Passwords do not match")
        .interact()
        .expect("Failed to read password");

    if let Err(problem) = swap_env::rpc_auth::validate_password_strength(&password) {
        panic!("{problem}");
    }

    let verifier = swap_env::rpc_auth::generate(&password);

    let _ = std::fs::remove_file(ASB_RPC_AUTH_FILE_ON_HOST);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(ASB_RPC_AUTH_FILE_ON_HOST)
        .and_then(|mut file| file.write_all(verifier.as_bytes()))
        .expect("Failed to write RPC auth keyfile");

    println!("Wrote RPC auth verifier to {ASB_RPC_AUTH_FILE_ON_HOST}");
    println!("Enter this password in asb-controller to access the RPC server.");
}
