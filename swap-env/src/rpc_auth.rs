use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::path::Path;

type HmacSha256 = Hmac<Sha256>;

const SALT_BYTES: usize = 16;
const DIGEST_BYTES: usize = 32;
const MIN_PASSWORD_LENGTH: usize = 16;

pub fn generate(password: &str) -> String {
    let mut salt = [0u8; SALT_BYTES];
    rand::thread_rng().fill_bytes(&mut salt);
    let salt = hex::encode(salt);
    let hmac = hash_with_salt(password, &salt);
    format!("{salt}:{hmac}")
}

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

pub fn load_verifier(path: &Path) -> Result<String> {
    let verifier = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read RPC auth file at {}", path.display()))?
        .trim()
        .to_string();

    if !is_well_formed(&verifier) {
        bail!("RPC auth file at {} is malformed", path.display());
    }

    Ok(verifier)
}

fn is_well_formed(verifier: &str) -> bool {
    verifier.split_once(':').is_some_and(|(salt, hmac)| {
        !salt.is_empty() && hex::decode(hmac).is_ok_and(|digest| digest.len() == DIGEST_BYTES)
    })
}

fn hash_with_salt(password: &str, salt: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(salt.as_bytes()).expect("HMAC accepts any key length");
    mac.update(password.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Rejects weak passwords. Requires length and a mix of character classes.
/// Only visible ASCII is allowed so the password is always a valid HTTP
/// header value.
pub fn validate_password_strength(password: &str) -> Result<(), String> {
    if !password.chars().all(|c| c.is_ascii_graphic()) {
        return Err(
            "Password must contain only visible ASCII characters (no whitespace, no non-ASCII symbols)"
                .to_string(),
        );
    }

    let mut missing = Vec::new();

    if password.len() < MIN_PASSWORD_LENGTH {
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
    if !password.chars().any(|c| c.is_ascii_punctuation()) {
        missing.push("a special character".to_string());
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
    fn well_formedness_requires_a_full_digest() {
        assert!(is_well_formed(&generate("pw")));
        assert!(!is_well_formed("salt:"));
        assert!(!is_well_formed("salt:deadbeef"));
        assert!(!is_well_formed("salt:not-hex"));
        assert!(!is_well_formed(":"));
        assert!(!is_well_formed("missing-colon"));
        let digest = "ab".repeat(DIGEST_BYTES);
        assert!(!is_well_formed(&format!(":{digest}")));
    }

    #[test]
    fn strength_rejects_weak_and_accepts_strong() {
        assert!(validate_password_strength("Sh0rt!").is_err());
        assert!(validate_password_strength("alllowercaseletters").is_err());
        assert!(validate_password_strength("has a space In It 9!").is_err());
        assert!(validate_password_strength("Sümb0l!Passphrase42x").is_err());
        assert!(validate_password_strength("C0ntr0l!Passphrase42\u{1}").is_err());
        assert!(validate_password_strength("Str0ng!Passphrase42xx").is_ok());
    }
}
