use crate::compose::ASB_RPC_AUTH_FILE_ON_HOST;

/// Prompts for a strong password and writes its `salt:hmac` verifier to the
/// host keyfile the generated compose mounts into the asb container.
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
    std::fs::write(ASB_RPC_AUTH_FILE_ON_HOST, &verifier).expect("Failed to write RPC auth keyfile");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            ASB_RPC_AUTH_FILE_ON_HOST,
            std::fs::Permissions::from_mode(0o600),
        )
        .expect("Failed to restrict permissions on RPC auth keyfile");
    }

    println!("Wrote RPC auth verifier to {ASB_RPC_AUTH_FILE_ON_HOST}");
    println!("Enter this password in asb-controller to access the RPC server.");
}
