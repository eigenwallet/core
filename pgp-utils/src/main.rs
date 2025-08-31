use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use anyhow::Context;
use pgp::composed::{Deserializable, SignedSecretKey};
use pgp_utils::PgpKey;

/// only for testing/development
fn main() -> anyhow::Result<()> {
    let mut key = if Path::new("key.secret").try_exists().unwrap_or(false) {
        let (key, _) = SignedSecretKey::from_armor_file("key.secret")
            .context("couldn't get secret key file")?;

        PgpKey::from(key)
    } else {
        let key = PgpKey::new()?;
        File::create("key.secret")?.write_all(key.private_key().as_bytes())?;

        key
    };

    File::create("key.public")?.write_all(key.public_key().as_bytes())?;
    let contact = File::open("contact.pgp")?.read_into_string()?;
    key.add_contact(contact).context("Couldn't add contact")?;

    let message = File::open("message.pgp")
        .context("couldn't get message file")?
        .read_into_string()?;

    println!("Message:\n========================\n{message}========================");
    println!("Verified: {}", key.verify(message)?);

    Ok(())
}

trait ReadExt: Read {
    fn read_into_string(&mut self) -> io::Result<String> {
        let mut buffer = String::new();
        self.read_to_string(&mut buffer)?;

        Ok(buffer)
    }
}

impl<T: Read> ReadExt for T {}
