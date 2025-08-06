use std::io::Write;
use tor_interface::legacy_tor_client::{LegacyTorClientConfig, LegacyTorClient};
use tor_interface::tor_crypto::Ed25519PrivateKey;
use tor_interface::tor_provider::{OnionListener, TorProvider};

fn main() {
    let mut client = LegacyTorClient::new(LegacyTorClientConfig::system_from_environment().expect("No configuration in the environment")).unwrap();
    client.bootstrap().unwrap();
    println!("{:?}", client.update().unwrap());

    let pk = Ed25519PrivateKey::generate();
    let ol = client.listener(&pk, 80, None, None).unwrap();
    println!("http://{}.onion", tor_interface::tor_crypto::V3OnionServiceId::from_private_key(&pk));

    loop {
        for u in client.update().unwrap() {
            println!("{:?}", u);
        }
        if let Some(mut peer) = ol.accept().unwrap() {
            println!("{:?}", &peer);
            peer.write_all(format!("HTTP/1.1 200 OK\r\n\r\n{:?}\n", peer).as_bytes()).unwrap();
        }
    }
}
