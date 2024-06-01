// stanndard
use std::io::{Read, Write};
#[cfg(feature = "arti-client-tor-provider")]
use std::sync::Arc;

// extern crates
use serial_test::serial;
#[cfg(feature = "arti-client-tor-provider")]
use tokio::runtime;

// internal crates
#[cfg(feature = "arti-client-tor-provider")]
use tor_interface::arti_client_tor_client::*;
#[cfg(feature = "legacy-tor-provider")]
use tor_interface::legacy_tor_client::*;
#[cfg(feature = "mock-tor-provider")]
use tor_interface::mock_tor_client::*;
use tor_interface::tor_crypto::*;
use tor_interface::tor_provider::*;

pub(crate) fn bootstrap_test(mut tor: Box<dyn TorProvider>) -> anyhow::Result<()> {
    tor.bootstrap()?;

    let mut received_log = false;
    let mut bootstrap_complete = false;
    while !bootstrap_complete {
        for event in tor.update()?.iter() {
            match event {
                TorEvent::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                TorEvent::BootstrapComplete => {
                    println!("Bootstrap Complete!");
                    bootstrap_complete = true;
                }
                TorEvent::LogReceived { line } => {
                    received_log = true;
                    println!("--- {}", line);
                }
                _ => {}
            }
        }
    }
    assert!(
        received_log,
        "should have received a log line from tor provider"
    );

    Ok(())
}

pub(crate) fn basic_onion_service_test(
    mut server_provider: Box<dyn TorProvider>,
    mut client_provider: Box<dyn TorProvider>,
) -> anyhow::Result<()> {
    server_provider.bootstrap()?;
    client_provider.bootstrap()?;

    let mut server_provider_bootstrap_complete = false;
    let mut client_provider_bootstrap_complete = false;

    while !server_provider_bootstrap_complete || !client_provider_bootstrap_complete {
        for event in server_provider.update()?.iter() {
            match event {
                TorEvent::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "Server Provider BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                TorEvent::BootstrapComplete => {
                    println!("Server Provider Bootstrap Complete!");
                    server_provider_bootstrap_complete = true;
                }
                TorEvent::LogReceived { line } => {
                    println!("--- {}", line);
                }
                _ => {}
            }
        }

        for event in client_provider.update()?.iter() {
            match event {
                TorEvent::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "Client Provider BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                TorEvent::BootstrapComplete => {
                    println!("Client Provider Bootstrap Complete!");
                    client_provider_bootstrap_complete = true;
                }
                TorEvent::LogReceived { line } => {
                    println!("--- {}", line);
                }
                _ => {}
            }
        }
    }

    // vanilla V3 onion service
    {
        let tor = &mut server_provider;

        // create an onion service for this test
        let private_key = Ed25519PrivateKey::generate();

        println!("Starting and listening to onion service");
        const VIRT_PORT: u16 = 42069u16;
        let listener = tor.listener(&private_key, VIRT_PORT, None)?;

        let mut onion_published = false;
        while !onion_published {
            for event in tor.update()?.iter() {
                match event {
                    TorEvent::LogReceived { line } => {
                        println!("--- {}", line);
                    }
                    TorEvent::OnionServicePublished { service_id } => {
                        let expected_service_id = V3OnionServiceId::from_private_key(&private_key);
                        if expected_service_id == *service_id {
                            println!("Onion Service {} published", service_id.to_string());
                            onion_published = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        const MESSAGE: &str = "Hello World!";

        {
            let tor = &mut client_provider;
            let service_id = V3OnionServiceId::from_private_key(&private_key);

            println!("Connecting to onion service");
            let mut attempt_count = 0;
            let mut client = loop {
                match tor.connect((service_id.clone(), VIRT_PORT).into(), None) {
                    Ok(client) => break client,
                    Err(err) => {
                        println!("connect error: {:?}", err);
                        attempt_count += 1;
                        if attempt_count == 3 {
                            panic!("failed to connect :(");
                        }
                    }
                }
            };
            println!("Client writing message: '{}'", MESSAGE);
            client.write_all(MESSAGE.as_bytes())?;
            client.flush()?;
            println!("End of client scope");
        }

        if let Some(mut server) = listener.accept()? {
            println!("Server reading message");
            let mut buffer = Vec::new();
            server.read_to_end(&mut buffer)?;
            let msg = String::from_utf8(buffer)?;

            assert_eq!(MESSAGE, msg);
            println!("Message received: '{}'", msg);
        } else {
            panic!("no listener");
        }
    }
    Ok(())
}

pub(crate) fn authenticated_onion_service_test(
    mut server_provider: Box<dyn TorProvider>,
    mut client_provider: Box<dyn TorProvider>,
) -> anyhow::Result<()> {
    server_provider.bootstrap()?;
    client_provider.bootstrap()?;

    let mut server_provider_bootstrap_complete = false;
    let mut client_provider_bootstrap_complete = false;

    while !server_provider_bootstrap_complete || !client_provider_bootstrap_complete {
        for event in server_provider.update()?.iter() {
            match event {
                TorEvent::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "Server Provider BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                TorEvent::BootstrapComplete => {
                    println!("Server Provider Bootstrap Complete!");
                    server_provider_bootstrap_complete = true;
                }
                TorEvent::LogReceived { line } => {
                    println!("--- {}", line);
                }
                _ => {}
            }
        }

        for event in client_provider.update()?.iter() {
            match event {
                TorEvent::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "Client Provider BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                TorEvent::BootstrapComplete => {
                    println!("Client Provider Bootstrap Complete!");
                    client_provider_bootstrap_complete = true;
                }
                TorEvent::LogReceived { line } => {
                    println!("--- {}", line);
                }
                _ => {}
            }
        }
    }

    // authenticated onion service
    {
        // create an onion service for this test
        let private_key = Ed25519PrivateKey::generate();

        let private_auth_key = X25519PrivateKey::generate();
        let public_auth_key = X25519PublicKey::from_private_key(&private_auth_key);

        println!("Starting and listening to authenticated onion service");
        const VIRT_PORT: u16 = 42069u16;
        let listener =
            server_provider.listener(&private_key, VIRT_PORT, Some(&[public_auth_key]))?;

        let mut onion_published = false;
        while !onion_published {
            for event in server_provider.update()?.iter() {
                match event {
                    TorEvent::LogReceived { line } => {
                        println!("--- {}", line);
                    }
                    TorEvent::OnionServicePublished { service_id } => {
                        let expected_service_id = V3OnionServiceId::from_private_key(&private_key);
                        if expected_service_id == *service_id {
                            println!(
                                "Authenticated Onion Service {} published",
                                service_id.to_string()
                            );
                            onion_published = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        const MESSAGE: &str = "Hello World!";

        {
            let service_id = V3OnionServiceId::from_private_key(&private_key);

            println!("Connecting to onion service (should fail)");
            assert!(
                client_provider
                    .connect((service_id.clone(), VIRT_PORT).into(), None)
                    .is_err(),
                "should not able to connect to an authenticated onion service without auth key"
            );

            println!("Add auth key for onion service");
            client_provider.add_client_auth(&service_id, &private_auth_key)?;

            println!("Connecting to onion service with authentication");
            let mut client =
                client_provider.connect((service_id.clone(), VIRT_PORT).into(), None)?;

            println!("Client writing message: '{}'", MESSAGE);
            client.write_all(MESSAGE.as_bytes())?;
            client.flush()?;
            println!("End of client scope");

            println!("Remove auth key for onion service");
            client_provider.remove_client_auth(&service_id)?;
        }

        if let Some(mut server) = listener.accept()? {
            println!("Server reading message");
            let mut buffer = Vec::new();
            server.read_to_end(&mut buffer)?;
            let msg = String::from_utf8(buffer)?;

            assert!(MESSAGE == msg);
            println!("Message received: '{}'", msg);
        } else {
            panic!("no listener");
        }
    }
    Ok(())
}

//
// Mock TorProvider tests
//

#[test]
#[cfg(feature = "mock-tor-provider")]
fn test_mock_bootstrap() -> anyhow::Result<()> {
    bootstrap_test(Box::new(MockTorClient::new()))
}

#[test]
#[cfg(feature = "mock-tor-provider")]
fn test_mock_onion_service() -> anyhow::Result<()> {
    let server_provider = Box::new(MockTorClient::new());
    let client_provider = Box::new(MockTorClient::new());
    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[cfg(feature = "mock-tor-provider")]
fn test_mock_authenticated_onion_service() -> anyhow::Result<()> {
    let server_provider = Box::new(MockTorClient::new());
    let client_provider = Box::new(MockTorClient::new());
    authenticated_onion_service_test(server_provider, client_provider)
}

//
// Legacy TorProvider tests
//

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_bootstrap() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_bootstrap");

    bootstrap_test(Box::new(LegacyTorClient::new(&tor_path, &data_path)?))
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_onion_service_server");
    let server_provider = Box::new(LegacyTorClient::new(&tor_path, &data_path)?);

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_onion_service_cient");
    let client_provider = Box::new(LegacyTorClient::new(&tor_path, &data_path)?);

    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_authenticated_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_authenticated_onion_service_server");
    let server_provider = Box::new(LegacyTorClient::new(&tor_path, &data_path)?);

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_authenticated_onion_service_cient");
    let client_provider = Box::new(LegacyTorClient::new(&tor_path, &data_path)?);

    authenticated_onion_service_test(server_provider, client_provider)
}

//
// Arti TorProvider tests
//

#[test]
#[serial]
#[cfg(feature = "arti-client-tor-provider")]
fn test_arti_client_bootstrap() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());
    let mut data_path = std::env::temp_dir();
    data_path.push("test_arti_bootstrap");
    let tor_provider = Box::new(ArtiClientTorClient::new(runtime, &data_path).unwrap());

    bootstrap_test(tor_provider)
}

#[test]
#[cfg(feature = "arti-client-tor-provider")]
fn test_arti_client_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());
    let mut data_path = std::env::temp_dir();
    data_path.push("test_arti_basic_onion_service_server");
    let server_provider = Box::new(ArtiClientTorClient::new(runtime.clone(), &data_path).unwrap());

    let mut data_path = std::env::temp_dir();
    data_path.push("test_arti_basic_onion_service_client");
    let client_provider = Box::new(ArtiClientTorClient::new(runtime.clone(), &data_path).unwrap());

    basic_onion_service_test(server_provider, client_provider)
}

/*
TODO: re-enable once client-auth is available in arti
#[test]
#[serial]
#[cfg(feature = "arti-client-tor-provider")]
fn test_arti_authenticated_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let mut data_path = std::env::temp_dir();
    data_path.push("test_arti_basic_onion_service_server");
    let server_provider = Box::new(ArtiClientTorClient::new(runtime.clone(), &data_path).unwrap());

    let mut data_path = std::env::temp_dir();
    data_path.push("test_arti_basic_onion_service_client");
    let client_provider = Box::new(ArtiClientTorClient::new(runtime.clone(), &data_path).unwrap());

    authenticated_onion_service_test(server_provider, client_provider)
}
*/

//
// Mixed Arti/Legacy TorProvider tests
//

#[test]
#[serial]
#[cfg(all(feature = "arti-client-tor-provider", feature = "legacy-tor-provider"))]
fn test_mixed_arti_client_legacy_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let mut data_path = std::env::temp_dir();
    data_path.push("test_arti_legacy_basic_onion_service_server");
    let server_provider = Box::new(ArtiClientTorClient::new(runtime, &data_path)?);

    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_arti_legacy_basic_onion_service_client");
    let client_provider = Box::new(LegacyTorClient::new(&tor_path, &data_path)?);

    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(all(feature = "arti-client-tor-provider", feature = "legacy-tor-provider"))]
fn test_mixed_legacy_arti_client_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_arty_basic_onion_service_client");
    let server_provider = Box::new(LegacyTorClient::new(&tor_path, &data_path)?);

    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_arti_basic_onion_service_server");
    let client_provider = Box::new(ArtiClientTorClient::new(runtime, &data_path)?);

    basic_onion_service_test(server_provider, client_provider)
}
