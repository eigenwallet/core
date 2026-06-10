use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const SALT_BYTES: usize = 16;
const MIN_PASSWORD_LENGTH: usize = 16;

/// Builds a `<salt>:<hmac>` verifier for the password using a fresh random salt.
pub fn generate(password: &str) -> String {
    let mut salt = [0u8; SALT_BYTES];
    rand::thread_rng().fill_bytes(&mut salt);
    let salt = hex::encode(salt);
    let hmac = hash_with_salt(password, &salt);
    format!("{salt}:{hmac}")
}

/// Constant-time check of a password against a `<salt>:<hmac>` verifier.
/// A malformed verifier never authenticates.
pub fn verify(password: &str, verifier: &str) -> bool {
    let Some((salt, expected)) = verifier.split_once(':') else {
        return false;
    };
    let Ok(expected) = hex::decode(expected) else {
        return false;
    };

    let mut mac = HmacSha256::new_from_slice(salt.as_bytes()).expect("HMAC accepts any key length");
    mac.update(password.as_bytes());
    mac.verify_slice(&expected).is_ok()
}

fn hash_with_salt(password: &str, salt: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(salt.as_bytes()).expect("HMAC accepts any key length");
    mac.update(password.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Rejects weak passwords. Requires length and a mix of character classes,
/// and forbids whitespace to keep the value unambiguous in an HTTP header.
pub fn validate_password_strength(password: &str) -> Result<(), String> {
    let mut missing = Vec::new();

    if password.chars().count() < MIN_PASSWORD_LENGTH {
        missing.push(format!("at least {MIN_PASSWORD_LENGTH} characters"));
    }
    if !password.chars().any(|c| c.is_ascii_lowercase()) {
        missing.push("a lowercase letter".to_string());
    }
    if !password.chars().any(|c| c.is_ascii_uppercase()) {
        missing.push("an uppercase letter".to_string());
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        missing.push("a digit".to_string());
    }
    if !password
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && !c.is_whitespace())
    {
        missing.push("a special character".to_string());
    }

    if password.chars().any(char::is_whitespace) {
        return Err("Password must not contain whitespace".to_string());
    }
    if missing.is_empty() {
        return Ok(());
    }

    Err(format!("Password is too weak; it must have {}", missing.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_then_verify_roundtrips() {
        let verifier = generate("Str0ng!Passphrase42xx");
        assert!(verify("Str0ng!Passphrase42xx", &verifier));
        assert!(!verify("not the password", &verifier));
    }

    #[test]
    fn verify_is_reproducible_for_a_fixed_salt() {
        let verifier = format!("deadbeef:{}", hash_with_salt("hunter2", "deadbeef"));
        assert!(verify("hunter2", &verifier));
        assert!(!verify("hunter3", &verifier));
    }

    #[test]
    fn malformed_verifiers_never_authenticate() {
        assert!(!verify("pw", "missing-colon"));
        assert!(!verify("pw", "salt:not-hex"));
        assert!(!verify("pw", ""));
    }

    #[test]
    fn strength_rejects_weak_and_accepts_strong() {
        assert!(validate_password_strength("Sh0rt!").is_err());
        assert!(validate_password_strength("alllowercaseletters").is_err());
        assert!(validate_password_strength("has a space In It 9!").is_err());
        assert!(validate_password_strength("Str0ng!Passphrase42xx").is_ok());
    }
}
