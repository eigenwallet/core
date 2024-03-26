// internal crates
use tor_interface::tor_crypto::*;

#[test]
fn test_ed25519() -> Result<(), anyhow::Error> {
    let private_key_blob = "ED25519-V3:rP3u8mZaKohap0lKsB8Z8qXbXqK456JKKGONDBhV+gPBVKa2mHVQqnRTVuFXe3inU3YW6qvc7glYEwe9rK0LhQ==";
    let private_raw: [u8; ED25519_PRIVATE_KEY_SIZE] = [
        0xacu8, 0xfdu8, 0xeeu8, 0xf2u8, 0x66u8, 0x5au8, 0x2au8, 0x88u8, 0x5au8, 0xa7u8, 0x49u8,
        0x4au8, 0xb0u8, 0x1fu8, 0x19u8, 0xf2u8, 0xa5u8, 0xdbu8, 0x5eu8, 0xa2u8, 0xb8u8, 0xe7u8,
        0xa2u8, 0x4au8, 0x28u8, 0x63u8, 0x8du8, 0x0cu8, 0x18u8, 0x55u8, 0xfau8, 0x03u8, 0xc1u8,
        0x54u8, 0xa6u8, 0xb6u8, 0x98u8, 0x75u8, 0x50u8, 0xaau8, 0x74u8, 0x53u8, 0x56u8, 0xe1u8,
        0x57u8, 0x7bu8, 0x78u8, 0xa7u8, 0x53u8, 0x76u8, 0x16u8, 0xeau8, 0xabu8, 0xdcu8, 0xeeu8,
        0x09u8, 0x58u8, 0x13u8, 0x07u8, 0xbdu8, 0xacu8, 0xadu8, 0x0bu8, 0x85u8,
    ];
    let public_raw: [u8; ED25519_PUBLIC_KEY_SIZE] = [
        0xf2u8, 0xfdu8, 0xa2u8, 0xdbu8, 0xf3u8, 0x80u8, 0xa6u8, 0xbau8, 0x74u8, 0xa4u8, 0x90u8,
        0xe1u8, 0x45u8, 0x55u8, 0xeeu8, 0xb9u8, 0x32u8, 0xa0u8, 0x5cu8, 0x39u8, 0x5au8, 0xe2u8,
        0x02u8, 0x83u8, 0x55u8, 0x27u8, 0x89u8, 0x6au8, 0x1fu8, 0x2fu8, 0x3du8, 0xc5u8,
    ];
    let public_base32 = "6L62FW7TQCTLU5FESDQUKVPOXEZKAXBZLLRAFA2VE6EWUHZPHXCQ====";
    let service_id_string = "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd";
    assert!(V3OnionServiceId::is_valid(&service_id_string));

    let mut message = [0x00u8; 256];
    let null_message = [0x00u8; 256];
    for (i, ptr) in message.iter_mut().enumerate() {
        *ptr = i as u8;
    }
    let signature_raw: [u8; ED25519_SIGNATURE_SIZE] = [
        0xa6u8, 0xd6u8, 0xc6u8, 0x1au8, 0x03u8, 0xbcu8, 0x43u8, 0x6fu8, 0x38u8, 0x53u8, 0x94u8,
        0xcdu8, 0xdcu8, 0x86u8, 0x0au8, 0x88u8, 0x64u8, 0x43u8, 0x1du8, 0x18u8, 0x84u8, 0x30u8,
        0x2fu8, 0xcdu8, 0xa6u8, 0x79u8, 0xcau8, 0x87u8, 0xd0u8, 0x29u8, 0xe7u8, 0x2bu8, 0x32u8,
        0x9bu8, 0xa2u8, 0xa4u8, 0x3cu8, 0x74u8, 0x6au8, 0x08u8, 0x67u8, 0x0eu8, 0x63u8, 0x60u8,
        0xcbu8, 0x46u8, 0x22u8, 0x55u8, 0x43u8, 0x5bu8, 0x84u8, 0x68u8, 0x0fu8, 0x47u8, 0xceu8,
        0x6cu8, 0xd2u8, 0xb8u8, 0xebu8, 0xfeu8, 0xf6u8, 0x9eu8, 0x97u8, 0x0au8,
    ];

    // test the golden path first
    let service_id = V3OnionServiceId::from_string(&service_id_string)?;

    let private_key = Ed25519PrivateKey::from_raw(&private_raw)?;
    assert_eq!(
        private_key,
        Ed25519PrivateKey::from_key_blob(&private_key_blob)?
    );
    assert_eq!(private_key_blob, private_key.to_key_blob());

    let public_key = Ed25519PublicKey::from_raw(&public_raw)?;
    assert_eq!(public_key, Ed25519PublicKey::from_service_id(&service_id)?);
    assert_eq!(public_key, Ed25519PublicKey::from_private_key(&private_key));
    assert_eq!(service_id, V3OnionServiceId::from_public_key(&public_key));
    assert_eq!(public_base32, public_key.to_base32());

    let signature = private_key.sign_message(&message);
    assert_eq!(signature, Ed25519Signature::from_raw(&signature_raw)?);
    assert!(signature.verify(&message, &public_key));
    assert!(!signature.verify(&null_message, &public_key));

    // some invalid service ids
    assert!(!V3OnionServiceId::is_valid(""));
    assert!(!V3OnionServiceId::is_valid(
        "
        aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ));
    assert!(!V3OnionServiceId::is_valid(
        "6L62FW7TQCTLU5FESDQUKVPOXEZKAXBZLLRAFA2VE6EWUHZPHXCZSJYD"
    ));

    // generate a new key, get the public key and sign/verify a message
    let private_key = Ed25519PrivateKey::generate();
    let public_key = Ed25519PublicKey::from_private_key(&private_key);
    let signature = private_key.sign_message(&message);
    assert!(signature.verify(&message, &public_key));

    // test invalid private key blob returns an error
    // https://gitlab.torproject.org/tpo/core/arti/-/issues/1021
    let private_raw: [u8; ED25519_PRIVATE_KEY_SIZE] = [
        0x2eu8, 0x26u8, 0x0au8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x0au8, 0x77u8, 0x77u8,
        0x77u8, 0x77u8, 0x5du8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8,
        0x82u8, 0xb4u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8,
        0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0xffu8,
        0xffu8, 0xffu8, 0xffu8, 0xffu8, 0xffu8, 0xffu8, 0xffu8, 0xffu8, 0xffu8, 0xffu8, 0xffu8,
        0xffu8, 0xffu8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x77u8, 0x82u8, 0x88u8,
    ];
    match Ed25519PrivateKey::from_raw(&private_raw) {
        Ok(_) => panic!("invalid key accepted"),
        Err(tor_interface::tor_crypto::Error::KeyInvalid) => (),
        Err(err) => panic!("unexpected error: {:?}", err),
    }

    Ok(())
}

#[test]
fn test_x25519() -> Result<(), anyhow::Error> {
    // private/public key pair
    const SECRET_BASE64: &str = "0GeSReJXdNcgvWRQdnDXhJGdu5UiwP2fefgT93/oqn0=";
    const SECRET_RAW: [u8; X25519_PRIVATE_KEY_SIZE] = [
        0xd0u8, 0x67u8, 0x92u8, 0x45u8, 0xe2u8, 0x57u8, 0x74u8, 0xd7u8, 0x20u8, 0xbdu8, 0x64u8,
        0x50u8, 0x76u8, 0x70u8, 0xd7u8, 0x84u8, 0x91u8, 0x9du8, 0xbbu8, 0x95u8, 0x22u8, 0xc0u8,
        0xfdu8, 0x9fu8, 0x79u8, 0xf8u8, 0x13u8, 0xf7u8, 0x7fu8, 0xe8u8, 0xaau8, 0x7du8,
    ];
    const PUBLIC_BASE32: &str = "AEXCBCEDJ5KU34YGGMZ7PVHVDEA7D7YB7VQAPJTMTZGRJLN3JASA";
    const PUBLIC_RAW: [u8; X25519_PUBLIC_KEY_SIZE] = [
        0x01u8, 0x2eu8, 0x20u8, 0x88u8, 0x83u8, 0x4fu8, 0x55u8, 0x4du8, 0xf3u8, 0x06u8, 0x33u8,
        0x33u8, 0xf7u8, 0xd4u8, 0xf5u8, 0x19u8, 0x01u8, 0xf1u8, 0xffu8, 0x01u8, 0xfdu8, 0x60u8,
        0x07u8, 0xa6u8, 0x6cu8, 0x9eu8, 0x4du8, 0x14u8, 0xadu8, 0xbbu8, 0x48u8, 0x24u8,
    ];

    // ensure we can convert from raw as expected
    assert_eq!(
        &X25519PrivateKey::from_raw(&SECRET_RAW)?.to_base64(),
        SECRET_BASE64
    );
    assert_eq!(
        &X25519PublicKey::from_raw(&PUBLIC_RAW).to_base32(),
        PUBLIC_BASE32
    );

    // ensure we can round-trip as expected
    assert_eq!(
        &X25519PrivateKey::from_base64(&SECRET_BASE64)?.to_base64(),
        SECRET_BASE64
    );
    assert_eq!(
        &X25519PublicKey::from_base32(&PUBLIC_BASE32)?.to_base32(),
        PUBLIC_BASE32
    );

    // ensure we generate the expected public key from private key
    let private_key = X25519PrivateKey::from_base64(&SECRET_BASE64)?;
    let public_key = X25519PublicKey::from_private_key(&private_key);
    assert_eq!(public_key.to_base32(), PUBLIC_BASE32);

    let message = b"All around me are familiar faces";

    let (signature, signbit) = private_key.sign_message(message)?;
    assert!(signature.verify_x25519(message, &public_key, signbit));

    Ok(())
}
