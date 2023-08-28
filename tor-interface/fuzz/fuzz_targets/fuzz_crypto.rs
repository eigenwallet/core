#![no_main]

// tor_interface
use tor_interface::tor_crypto::*;

// fuzzing
use libfuzzer_sys::fuzz_target;
use libfuzzer_sys::arbitrary;
use libfuzzer_sys::arbitrary::Arbitrary;

#[derive(Arbitrary, Debug)]
struct CryptoData<'a> {
    ed25519_public_raw: [u8; 32],
    onion_service_id: &'a str,
    x25519_public_raw: [u8; 32],
    message_1: &'a [u8],
    message_2: &'a [u8],
    ed25519_private_raw_1: [u8; 64],
    ed25519_private_raw_2: [u8; 64],
    x25519_private_raw_1: [u8; 32],
    x25519_private_raw_2: [u8; 32],
}

fuzz_target!(|data: CryptoData| {

    //
    // ed25519 tests
    //

    // ensure random bytes don't break ed25519public from_raw
    let _ = Ed25519PublicKey::from_raw(&data.ed25519_public_raw);

    // ensure random string doesn't break v3onionserviceid from_string
    let _ = V3OnionServiceId::from_string(data.onion_service_id);

    // ensure random bytes don't break x25519public from_raw
    let _ = X25519PublicKey::from_raw(&data.x25519_public_raw);

    // try to build key from raw binary blob, return early if invalid
    if let Ok(ed25519_private_1) = Ed25519PrivateKey::from_raw(&data.ed25519_private_raw_1) {
        // ensure key round-trips through keyblob representation
        assert_eq!(Ed25519PrivateKey::from_key_blob(ed25519_private_1.to_key_blob().as_ref()).unwrap(), ed25519_private_1);

        // ensure key round-trips through raw bytes representation
        match Ed25519PrivateKey::from_raw(&ed25519_private_1.to_bytes()) {
            Ok(ed25519_private) => assert_eq!(ed25519_private, ed25519_private_1),
            Err(err) => panic!("{:?}", err),
        }

        // derive private keys public key
        let ed25519_public_1 = Ed25519PublicKey::from_private_key(&ed25519_private_1);

        // compare onion service id derivation from public vs privat ekey
        assert_eq!(V3OnionServiceId::from_private_key(&ed25519_private_1), V3OnionServiceId::from_public_key(&ed25519_public_1));
        let onion_service_id_1 = V3OnionServiceId::from_public_key(&ed25519_public_1);
        // ensure service id round-trips through string representation
        assert_eq!(V3OnionServiceId::from_string(&onion_service_id_1.to_string()).unwrap(), onion_service_id_1);

        // ensure public key round-trips through service id
        assert_eq!(ed25519_public_1, Ed25519PublicKey::from_service_id(&V3OnionServiceId::from_public_key(&ed25519_public_1)).unwrap());

        // ensure key round-trips through raw bytes representation
        assert_eq!(ed25519_public_1, Ed25519PublicKey::from_raw(ed25519_public_1.as_bytes()).unwrap());

        // sign and verify a message
        let ed25519_signature_1 = ed25519_private_1.sign_message(data.message_1);
        assert!(ed25519_signature_1.verify(data.message_1, &ed25519_public_1));
        // verify signature does not work for unrelated message
        if data.message_1 != data.message_2 {
            assert!(!ed25519_signature_1.verify(data.message_2, &ed25519_public_1));
        }

        // ensure we can't verfify another key's signature
        if data.ed25519_private_raw_1 != data.ed25519_private_raw_2 {
            // try to build key from raw binary blob, return early if invalid
            if let Ok(ed25519_private_2) = Ed25519PrivateKey::from_raw(&data.ed25519_private_raw_2) {

                // ensure key round-trips through keyblob representation
                assert_eq!(Ed25519PrivateKey::from_key_blob(ed25519_private_2.to_key_blob().as_ref()).unwrap(), ed25519_private_2);

                // ensure key round-trips through raw bytes representation
                match Ed25519PrivateKey::from_raw(&ed25519_private_2.to_bytes()) {
                    Ok(ed25519_private) => assert_eq!(ed25519_private, ed25519_private_2),
                    Err(err) => panic!("{:?}", err),
                }

                // derive private key's public key
                let ed25519_public_2 = Ed25519PublicKey::from_private_key(&ed25519_private_2);

                // compare onion service id derivation from public vs privat ekey
                assert_eq!(V3OnionServiceId::from_private_key(&ed25519_private_2), V3OnionServiceId::from_public_key(&ed25519_public_2));
                let onion_service_id_2 = V3OnionServiceId::from_public_key(&ed25519_public_2);
                // ensure service id round-trips through string representation
                assert_eq!(V3OnionServiceId::from_string(&onion_service_id_2.to_string()).unwrap(), onion_service_id_2);

                // ensure public key round-trips through service id
                assert_eq!(ed25519_public_2, Ed25519PublicKey::from_service_id(&V3OnionServiceId::from_public_key(&ed25519_public_2)).unwrap());

                // ensure key round-trips through raw bytes representation
                assert_eq!(ed25519_public_2, Ed25519PublicKey::from_raw(ed25519_public_2.as_bytes()).unwrap());


                // sign and verify a message
                let ed25519_signature_2 = ed25519_private_2.sign_message(data.message_2);
                assert!(ed25519_signature_2.verify(data.message_2, &ed25519_public_2));

                // verify signature does not work for unrelated message
                if data.message_1 != data.message_2 {
                    assert!(!ed25519_signature_2.verify(data.message_1, &ed25519_public_2));
                }

                // verify we cannot verify signatures using the wrong public keys
                if ed25519_public_1 != ed25519_public_2 {
                    assert!(!ed25519_signature_1.verify(data.message_1, &ed25519_public_2));
                    assert!(!ed25519_signature_2.verify(data.message_2, &ed25519_public_1));
                }
            }
        }
    }

    //
    // x25519 tests
    //

    if let Ok(x25519_private_1) = X25519PrivateKey::from_raw(&data.x25519_private_raw_1) {
        // ensure round-trips through byte representation
        assert_eq!(x25519_private_1, X25519PrivateKey::from_raw(&x25519_private_1.to_bytes()).unwrap());
        assert_eq!(data.x25519_private_raw_1, x25519_private_1.to_bytes());
        // ensure round-trips through base64 representation
        assert_eq!(x25519_private_1, X25519PrivateKey::from_base64(&x25519_private_1.to_base64()).unwrap());

        // ensure converts to e25519 without issue
        let _ = Ed25519PrivateKey::from_private_x25519(&x25519_private_1).unwrap();

        let x25519_public_1 = X25519PublicKey::from_private_key(&x25519_private_1);
        // ensure round-trips through byte representation
        assert_eq!(x25519_public_1, X25519PublicKey::from_raw(x25519_public_1.as_bytes()));
        // ensure round-trips through base32 representation
        assert_eq!(x25519_public_1, X25519PublicKey::from_base32(&x25519_public_1.to_base32()).unwrap());

        if let Ok(x25519_private_2) = X25519PrivateKey::from_raw(&data.x25519_private_raw_2) {
            // ensure round-trips through byte representation
            assert_eq!(x25519_private_2, X25519PrivateKey::from_raw(&x25519_private_2.to_bytes()).unwrap());
            assert_eq!(data.x25519_private_raw_2, x25519_private_2.to_bytes());
            // ensure round-trips through base64 representation
            assert_eq!(x25519_private_2, X25519PrivateKey::from_base64(&x25519_private_2.to_base64()).unwrap());

            // ensure converts to e25519 without issue
            let _ = Ed25519PrivateKey::from_private_x25519(&x25519_private_2).unwrap();

            let x25519_public_2 = X25519PublicKey::from_private_key(&x25519_private_2);
            // ensure round-trips through byte representation
            assert_eq!(x25519_public_2, X25519PublicKey::from_raw(x25519_public_2.as_bytes()));
            // ensure round-trips through base32 representation
            assert_eq!(x25519_public_2, X25519PublicKey::from_base32(&x25519_public_2.to_base32()).unwrap());
        }
    }


});
