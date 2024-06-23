// stanndard
#[cfg(feature = "legacy-tor-provider")]
use std::fs::File;
use std::io::{Read, Write};
#[cfg(feature = "legacy-tor-provider")]
use std::process;
#[cfg(feature = "legacy-tor-provider")]
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
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

    let tor_config = LegacyTorClientConfig::BundledTor{
        tor_bin_path: tor_path,
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: None,
        bridge_lines: None,
    };

    bootstrap_test(Box::new(LegacyTorClient::new(tor_config)?))
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_pluggable_transport_bootstrap() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_pluggable_transport_bootstrap");


    // find the lyrebird bin
    let teb_path = std::env::var("TEB_PATH")?;
    if teb_path.is_empty() {
        println!("TEB_PATH environment variable empty, so skipping test_legacy_pluggable_transport_bootstrap()");
        return Ok(());
    }
    let mut lyrebird_path = std::path::PathBuf::from(&teb_path);
    let lyrebird_bin = format!("lyrebird{}", std::env::consts::EXE_SUFFIX);
    lyrebird_path.push(lyrebird_bin.clone());
    assert!(std::path::Path::exists(&lyrebird_path));
    assert!(std::path::Path::is_file(&lyrebird_path));

    // configure lyrebird pluggable transport
    let pluggable_transport = PluggableTransportConfig::new(
        vec!["obfs4".to_string()],
        lyrebird_path)?;

    // obfs4 bridgeline
    let bridge_line = BridgeLine::from_str("obfs4 207.172.185.193:22223 F34AC0CDBC06918E54292A474578C99834A58893 cert=MjqosoyVylLQuLo4LH+eQ5hS7Z44s2CaMfQbIjJtn4bGRnvLv8ldSvSED5JpvWSxm09XXg iat-mode=0")?;

    let tor_config = LegacyTorClientConfig::BundledTor{
        tor_bin_path: tor_path,
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: Some(vec![pluggable_transport]),
        bridge_lines: Some(vec![bridge_line]),
    };

    bootstrap_test(Box::new(LegacyTorClient::new(tor_config)?))
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_onion_service_server");
    let tor_config = LegacyTorClientConfig::BundledTor{
        tor_bin_path: tor_path.clone(),
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: None,
        bridge_lines: None,
    };
    let server_provider = Box::new(LegacyTorClient::new(tor_config)?);

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_onion_service_cient");
    let tor_config = LegacyTorClientConfig::BundledTor{
        tor_bin_path: tor_path,
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: None,
        bridge_lines: None,
    };
    let client_provider = Box::new(LegacyTorClient::new(tor_config)?);

    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_authenticated_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_authenticated_onion_service_server");
    let tor_config = LegacyTorClientConfig::BundledTor{
        tor_bin_path: tor_path.clone(),
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: None,
        bridge_lines: None,
    };
    let server_provider = Box::new(LegacyTorClient::new(tor_config)?);

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_authenticated_onion_service_cient");
    let tor_config = LegacyTorClientConfig::BundledTor{
        tor_bin_path: tor_path,
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: None,
        bridge_lines: None,
    };
    let client_provider = Box::new(LegacyTorClient::new(tor_config)?);


    authenticated_onion_service_test(server_provider, client_provider)
}

//
// System Legacy TorProvider tests
//

#[cfg(test)]
fn start_system_tor_daemon(tor_path: &std::ffi::OsStr, name: &str, control_port: u16, socks_port: u16) -> anyhow::Result<Child> {

    let mut data_path = std::env::temp_dir();
    data_path.push(name);
    std::fs::create_dir_all(&data_path)?;
    let default_torrc = data_path.join("default_torrc");
    { let _ = File::create(&default_torrc)?; }
    let torrc = data_path.join("torrc");
    { let _ = File::create(&torrc)?; }

    let tor_daemon = Command::new(tor_path)
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        // point to our above written torrc file
        .arg("--defaults-torrc")
        .arg(default_torrc)
        // location of torrc
        .arg("--torrc-file")
        .arg(torrc)
        // enable networking
        .arg("DisableNetwork")
        .arg("0")
        // root data directory
        .arg("DataDirectory")
        .arg(data_path)
        // daemon will assign us a port, and we will
        // read it from the control port file
        .arg("ControlPort")
        .arg(control_port.to_string())
        // password: foobar1
        .arg("HashedControlPassword")
        .arg("16:E807DCE69AFE9979600760C9758B95ADB2F95E8740478AEA5356C95358")
        // socks port
        .arg("SocksPort")
        .arg(socks_port.to_string())
        // tor process will shut down after this process shuts down
        // to avoid orphaned tor daemon
        .arg("__OwningControllerProcess")
        .arg(process::id().to_string())
        .spawn()?;


    Ok(tor_daemon)
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_system_legacy_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;

    let mut server_tor_daemon = start_system_tor_daemon(tor_path.as_os_str(), "test_system_legacy_onion_service_server", 9251u16, 9250u16)?;
    let mut client_tor_daemon = start_system_tor_daemon(tor_path.as_os_str(), "test_system_legacy_onion_service_client", 9351u16, 9350u16)?;

    // give daemons time to start
    std::thread::sleep(std::time::Duration::from_secs(5));

    let tor_config = LegacyTorClientConfig::SystemTor{
        tor_socks_addr: std::net::SocketAddr::from_str("127.0.0.1:9250")?,
        tor_control_addr: std::net::SocketAddr::from_str("127.0.0.1:9251")?,
        tor_control_passwd: "password".to_string(),
    };
    let server_provider = Box::new(LegacyTorClient::new(tor_config)?);

    let tor_config = LegacyTorClientConfig::SystemTor{
        tor_socks_addr: std::net::SocketAddr::from_str("127.0.0.1:9350")?,
        tor_control_addr: std::net::SocketAddr::from_str("127.0.0.1:9351")?,
        tor_control_passwd: "password".to_string(),
    };
    let client_provider = Box::new(LegacyTorClient::new(tor_config)?);

    basic_onion_service_test(server_provider, client_provider)?;

    server_tor_daemon.kill()?;
    client_tor_daemon.kill()?;

    Ok(())
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_system_legacy_authenticated_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;

    let mut server_tor_daemon = start_system_tor_daemon(tor_path.as_os_str(), "test_system_legacy_authenticated_onion_service_server", 9251u16, 9250u16)?;
    let mut client_tor_daemon = start_system_tor_daemon(tor_path.as_os_str(), "test_system_legacy_authenticated_onion_service_client", 9351u16, 9350u16)?;

    // give daemons time to start
    std::thread::sleep(std::time::Duration::from_secs(5));

    let tor_config = LegacyTorClientConfig::SystemTor{
        tor_socks_addr: std::net::SocketAddr::from_str("127.0.0.1:9250")?,
        tor_control_addr: std::net::SocketAddr::from_str("127.0.0.1:9251")?,
        tor_control_passwd: "password".to_string(),
    };
    let server_provider = Box::new(LegacyTorClient::new(tor_config)?);

    let tor_config = LegacyTorClientConfig::SystemTor{
        tor_socks_addr: std::net::SocketAddr::from_str("127.0.0.1:9350")?,
        tor_control_addr: std::net::SocketAddr::from_str("127.0.0.1:9351")?,
        tor_control_passwd: "password".to_string(),
    };
    let client_provider = Box::new(LegacyTorClient::new(tor_config)?);

    authenticated_onion_service_test(server_provider, client_provider)?;

    server_tor_daemon.kill()?;
    client_tor_daemon.kill()?;

    Ok(())
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
    let tor_config = LegacyTorClientConfig::BundledTor{
        tor_bin_path: tor_path,
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: None,
        bridge_lines: None,
    };
    let client_provider = Box::new(LegacyTorClient::new(tor_config)?);

    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(all(feature = "arti-client-tor-provider", feature = "legacy-tor-provider"))]
fn test_mixed_legacy_arti_client_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_arty_basic_onion_service_client");
    let tor_config = LegacyTorClientConfig::BundledTor{
        tor_bin_path: tor_path,
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: None,
        bridge_lines: None,
    };
    let server_provider = Box::new(LegacyTorClient::new(tor_config)?);

    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let mut data_path = std::env::temp_dir();
    data_path.push("test_legacy_arti_basic_onion_service_server");
    let client_provider = Box::new(ArtiClientTorClient::new(runtime, &data_path)?);

    basic_onion_service_test(server_provider, client_provider)
}

//
// Misc Utils
//

#[test]
fn test_tor_provider_target_addr() -> anyhow::Result<()> {
    let valid_ip_addr: &[&str] = &[
        "192.168.1.1:80",
        "10.0.0.1:443",
        "172.16.0.1:8080",
        "8.8.8.8:53",
        "255.255.255.255:65535",
        "0.0.0.0:22",
        "192.168.0.254:21",
        "127.0.0.1:3306",
        "1.1.1.1:123",
        "224.0.0.1:554",
        "169.254.0.1:179",
        "203.0.113.1:80",
        "198.51.100.1:443",
        "100.64.0.1:8080",
        "192.0.2.1:53",
        "192.88.99.1:22",
        "192.0.0.1:21",
        "240.0.0.1:3306",
        "198.18.0.1:123",
        "233.252.0.1:554",
        "[2001:0db8:85a3:0000:0000:8a2e:0370:7334]:80",
        "[2001:db8:85a3::8a2e:370:7334]:443",
        "[::1]:8080",
        "[::ffff:192.168.1.1]:53",
        "[2001:0db8::1]:22",
        "[fe80::1ff:fe23:4567:890a]:21",
        "[2001:db8::1:0:0:1]:3306",
        "[2001:0db8:0000:0042:0000:8a2e:0370:7334]:123",
        "[ff02::1]:554",
        "[fe80::abcd:ef01:2345:6789]:179",
        "[2001:0db8:85a3:0000:0000:8a2e:0370:7334]:80",
        "[2001:db8:85a3::8a2e:370:7334]:443",
        "[::1]:8080",
        "[::ffff:c0a8:101]:53",
        "[2001:db8::1:0:0:1]:22",
        "[fe80::1ff:fe23:4567:890a]:21",
        "[2001:db8:0000:0042:0000:8a2e:0370:7334]:3306",
        "[ff02::1]:123",
        "[fe80::abcd:ef01:2345:6789]:554",
        "[2001:db8::1]:179",
    ];

    for target_addr_str in valid_ip_addr {
        match TargetAddr::from_str(target_addr_str) {
            Ok(TargetAddr::Ip(socket_addr)) => println!("{} => {}", target_addr_str, socket_addr),
            Ok(TargetAddr::OnionService(onion_addr)) => panic!(
                "unexpected conversion: {} => OnionService({})",
                target_addr_str, onion_addr
            ),
            Ok(TargetAddr::Domain(domain_addr)) => panic!(
                "unexpected conversion: {} => DomainAddr({})",
                target_addr_str, domain_addr
            ),
            Err(err) => Err(err)?,
        }
    }

    let valid_onion_addr: &[&str] = &[
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion:65535",
        "6L62FW7TQCTLU5FESDQUKVPOXEZKAXBZLLRAFA2VE6EWUHZPHXCZSJYD.onion:1",
    ];

    for target_addr_str in valid_onion_addr {
        match TargetAddr::from_str(target_addr_str) {
            Ok(TargetAddr::Ip(socket_addr)) => panic!(
                "unexpected conversion: {} => Ip({})",
                target_addr_str, socket_addr
            ),
            Ok(TargetAddr::OnionService(onion_addr)) => {
                println!("{} => {}", target_addr_str, onion_addr)
            }
            Ok(TargetAddr::Domain(domain_addr)) => panic!(
                "unexpected conversion: {} => DomainAddr({})",
                target_addr_str, domain_addr
            ),
            Err(err) => Err(err)?,
        }
    }

    let valid_domain_addr: &[&str] = &[
        "example.com:80",
        "subdomain.example.com:443",
        "xn--e1afmkfd.xn--p1ai:8080",       // domain in Punycode for "пример.рф"
        "xn--fsqu00a.xn--0zwm56d:53",       // domain in Punycode for "例子.测试"
        "münich.com:22",                    // domain with UTF-8 characters
        "xn--mnich-kva.com:21",             // Punycode for "münich.com"
        "exämple.com:3306",                 // domain with UTF-8 characters
        "xn--exmple-cua.com:123",           // Punycode for "exämple.com"
        "例子.com:554",                      // domain with UTF-8 characters
        "xn--fsqu00a.com:179",              // Punycode for "例子.com"
        "täst.de:80",                       // domain with UTF-8 characters
        "xn--tst-qla.de:443",               // Punycode for "täst.de"
        "xn--fiqs8s:80",                    // Punycode for "中国"
        "xn--wgbh1c:8080",                  // Punycode for "مصر"
        "münster.de:22",                    // domain with UTF-8 characters
        "xn--mnster-3ya.de:21",             // Punycode for "münster.de"
        "bücher.com:3306",                  // domain with UTF-8 characters
        "xn--bcher-kva.com:123",            // Punycode for "bücher.com"
        "xn--vermgensberatung-pwb.com:554", // Punycode for "vermögensberatung.com"
        // Max Length
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.ddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd:65535"
    ];

    for target_addr_str in valid_domain_addr {
        match TargetAddr::from_str(target_addr_str) {
            Ok(TargetAddr::Ip(socket_addr)) => panic!(
                "unexpected conversion: {} => SocketAddr({})",
                target_addr_str, socket_addr
            ),
            Ok(TargetAddr::OnionService(onion_addr)) => panic!(
                "unexpected conversion: {} => OnionService({})",
                target_addr_str, onion_addr
            ),
            Ok(TargetAddr::Domain(domain_addr)) => {
                println!("{} => {}", target_addr_str, domain_addr)
            }
            Err(err) => Err(err)?,
        }
    }

    let invalid_target_addr: &[&str] = &[
        // ipv4-ish
        "192.168.1.1:99999", // Port number out of range
        "192.168.1.1:abc",   // Invalid port number
        "192.168.1.1:",      // Missing port number
        "192.168.1.1: 80",   // Space in port number
        "192.168.1.1:80a",   // Non-numeric characters in port number
        // ipv6-ish
        "[2001:db8:::1]:80",                            // Triple colons
        "[2001:db8:85a3::8a2e:370:7334:1234::abcd]:80", // Too many groups
        "[2001:db8:85a3::8a2e:370g:7334]:80",           // Invalid character in group
        "[2001:db8:85a3::8a2e:370:7334]:99999",         // Port number out of range
        "[2001:db8:85a3:8a2e:370:7334]:80",             // Missing double colons
        "[::12345]:80",                                 // Excessive leading zeroes
        "[2001:db8:85a3::8a2e:370:7334:]:80",           // Trailing colon
        "[2001:db8:85a3::8a2e:370:7334]",               // Missing port number
        "2001:db8:85a3::8a2e:370:7334:80",              // Missing square brackets
        "[2001:db8:85a3::8a2e:370:7334]: 80",           // Space in port number
        "[2001:db8:85a3::8a2e:370:7334]:80a",           // Non-numeric characters in port number
        // onion service-ish
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd234567.onion:80", // Too long for v3
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxcz.onion:443", // Too short for v3
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion:99999", // Port number out of range
        "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrst.onion:21", // Invalid characters
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion:abc", // Invalid port number
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion: 80", // Space in port number
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion:80a", // Non-numeric characters in port number
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion:80",  // Invalid service id
        // domain-ish
        "example..com:80",        // Double dots
        "exa mple.com:53",        // Space in domain
        "example.com:99999",      // Port number out of range
        "exaample.com:abc",       // Invalid port number
        "exaample.com:",          // Missing port number
        "exaample.com: 80",       // Space in port number
        "ex@mple.com:80",         // Special character in domain
        "example.com:80a",        // Non-numeric characters in port number
        "exämple..com:80",        // UTF-8 with double dot
        "xn--exmple-cua.com: 80", // Punycode with space in port number
        "xn--exmple-cua.com:80a", // Punycode with non-numeric port
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.com:65535", // Label too long
    ];

    for target_addr_str in invalid_target_addr {
        match TargetAddr::from_str(target_addr_str) {
            Ok(TargetAddr::Ip(socket_addr)) => panic!(
                "unexpected conversion: {} => SocketAddr({})",
                target_addr_str, socket_addr
            ),
            Ok(TargetAddr::OnionService(onion_addr)) => panic!(
                "unexpected conversion: {} => OnionService({})",
                target_addr_str, onion_addr
            ),
            Ok(TargetAddr::Domain(domain_addr)) => panic!(
                "unexpected conversion: {} => DomainAddr({})",
                target_addr_str, domain_addr
            ),
            Err(_) => (),
        }
    }

    Ok(())
}
