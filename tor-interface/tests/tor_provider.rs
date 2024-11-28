// stanndard
#[cfg(feature = "legacy-tor-provider")]
use std::fs::File;
use std::io::{Read, Write};
#[cfg(feature = "legacy-tor-provider")]
use std::ops::Drop;
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
#[cfg(feature = "arti-tor-provider")]
use tor_interface::arti_tor_client::*;
#[cfg(feature = "legacy-tor-provider")]
use tor_interface::censorship_circumvention::*;
#[cfg(feature = "legacy-tor-provider")]
use tor_interface::legacy_tor_client::*;
#[cfg(feature = "mock-tor-provider")]
use tor_interface::mock_tor_client::*;
use tor_interface::tor_crypto::*;
use tor_interface::tor_provider::*;

//
// TorProvider Factory Functions
//

// purely in-process mock tor provider
#[cfg(test)]
#[cfg(feature = "mock-tor-provider")]
fn build_mock_tor_provider() -> anyhow::Result<Box<dyn TorProvider>> {
    Ok(Box::new(MockTorClient::new()))
}

// out-of-process c-tor owned by this process
#[cfg(test)]
#[cfg(feature = "legacy-tor-provider")]
fn build_bundled_legacy_tor_provider(name: &str) -> anyhow::Result<Box<dyn TorProvider>> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;
    let mut data_path = std::env::temp_dir();
    data_path.push(name);

    let tor_config = LegacyTorClientConfig::BundledTor {
        tor_bin_path: tor_path,
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: None,
        bridge_lines: None,
    };

    Ok(Box::new(LegacyTorClient::new(tor_config)?))
}

// out-of-process pt-using c-tor owned  by this process
#[cfg(test)]
#[cfg(feature = "legacy-tor-provider")]
fn build_bundled_pt_legacy_tor_provider(name: &str) -> anyhow::Result<Option<Box<dyn TorProvider>>> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;
    let mut data_path = std::env::temp_dir();
    data_path.push(name);

    // find the lyrebird bin
    let teb_path = std::env::var("TEB_PATH")?;
    if teb_path.is_empty() {
        println!("TEB_PATH environment variable empty, so skipping test_legacy_pluggable_transport_bootstrap()");
        return Ok(None);
    }
    let mut lyrebird_path = std::path::PathBuf::from(&teb_path);
    let lyrebird_bin = format!("lyrebird{}", std::env::consts::EXE_SUFFIX);
    lyrebird_path.push(lyrebird_bin.clone());
    assert!(std::path::Path::exists(&lyrebird_path));
    assert!(std::path::Path::is_file(&lyrebird_path));

    // configure lyrebird pluggable transport
    let pluggable_transport =
        PluggableTransportConfig::new(vec!["obfs4".to_string()], lyrebird_path)?;

    // obfs4 bridgeline
    let bridge_line = BridgeLine::from_str("obfs4 207.172.185.193:22223 F34AC0CDBC06918E54292A474578C99834A58893 cert=MjqosoyVylLQuLo4LH+eQ5hS7Z44s2CaMfQbIjJtn4bGRnvLv8ldSvSED5JpvWSxm09XXg iat-mode=0")?;

    let tor_config = LegacyTorClientConfig::BundledTor {
        tor_bin_path: tor_path,
        data_directory: data_path,
        proxy_settings: None,
        allowed_ports: None,
        pluggable_transports: Some(vec![pluggable_transport]),
        bridge_lines: Some(vec![bridge_line]),
    };

    Ok(Some(Box::new(LegacyTorClient::new(tor_config)?)))
}

#[cfg(feature = "legacy-tor-provider")]
struct TorProcess {child: Child}
#[cfg(feature = "legacy-tor-provider")]
impl Drop for TorProcess {
    fn drop(&mut self) -> () {
        let _ = self.child.kill();
    }
}

#[cfg(test)]
#[cfg(feature = "legacy-tor-provider")]
fn build_system_legacy_tor_provider(
    name: &str,
    control_port: u16,
    socks_port: u16,
) -> anyhow::Result<(Box<dyn TorProvider>, TorProcess)> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;

    let mut data_path = std::env::temp_dir();
    data_path.push(name);
    std::fs::create_dir_all(&data_path)?;
    let default_torrc = data_path.join("default_torrc");
    {
        let _ = File::create(&default_torrc)?;
    }
    let torrc = data_path.join("torrc");
    {
        let _ = File::create(&torrc)?;
    }

    let tor_daemon = TorProcess { child: Command::new(tor_path)
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
        // control port
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
        .spawn()?
    };
    // give daemons time to start
    std::thread::sleep(std::time::Duration::from_secs(5));

    let tor_config = LegacyTorClientConfig::SystemTor {
        tor_socks_addr: std::net::SocketAddr::from_str(format!("127.0.0.1:{socks_port}").as_str())?,
        tor_control_addr: std::net::SocketAddr::from_str(format!("127.0.0.1:{control_port}").as_str())?,
        tor_control_passwd: "password".to_string(),
    };
    let tor_provider = Box::new(LegacyTorClient::new(tor_config)?);

    Ok((tor_provider, tor_daemon))
}

#[cfg(test)]
#[cfg(feature = "arti-client-tor-provider")]
fn build_arti_client_tor_provider(runtime: Arc<runtime::Runtime>, name: &str) -> anyhow::Result<Box<dyn TorProvider>> {

    let mut data_path = std::env::temp_dir();
    data_path.push(name);
    Ok(Box::new(ArtiClientTorClient::new(runtime, &data_path)?))
}

#[cfg(test)]
#[cfg(feature = "arti-tor-provider")]
fn build_arti_tor_provider(name: &str) -> anyhow::Result<Box<dyn TorProvider>> {
    let arti_path = which::which(format!("arti{}", std::env::consts::EXE_SUFFIX))?;
    let mut data_path = std::env::temp_dir();
    data_path.push(name);

    let arti_config = ArtiTorClientConfig::BundledArti {
        arti_bin_path: arti_path,
        data_directory: data_path,
    };

    Ok(Box::new(ArtiTorClient::new(arti_config)?))
}
//
// Test Functions
//

#[allow(dead_code)]
pub(crate) fn bootstrap_test(mut tor: Box<dyn TorProvider>, skip_connect_tests: bool) -> anyhow::Result<()> {
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

    //
    // Attempt to connect to various endpoints
    //

    if !skip_connect_tests {

        // example.com
        let stream = tor.connect(TargetAddr::from_str("www.example.com:80")?, None)?;
        println!("stream: {stream:?}");

        // google dns (ipv4)
        let stream = tor.connect(TargetAddr::from_str("8.8.8.8:53")?, None)?;
        println!("stream: {stream:?}");

        // google dns (ipv6)
        let stream = tor.connect(TargetAddr::from_str("[2001:4860:4860::8888]:53")?, None)?;
        println!("stream: {stream:?}");

        // riseup onion service
        let stream = tor.connect(TargetAddr::from_str("vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd.onion:80")?, None)?;
        println!("stream: {stream:?}");

    }

    Ok(())
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
            let mut attempt_count = 0;
            let mut client = loop {
                match client_provider.connect((service_id.clone(), VIRT_PORT).into(), None) {
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
    bootstrap_test(build_mock_tor_provider()?, true)
}

#[test]
#[cfg(feature = "mock-tor-provider")]
fn test_mock_onion_service() -> anyhow::Result<()> {
    let server_provider = build_mock_tor_provider()?;
    let client_provider = build_mock_tor_provider()?;
    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[cfg(feature = "mock-tor-provider")]
fn test_mock_authenticated_onion_service() -> anyhow::Result<()> {
    let server_provider = build_mock_tor_provider()?;
    let client_provider = build_mock_tor_provider()?;
    authenticated_onion_service_test(server_provider, client_provider)
}

//
// Legacy TorProvider tests
//

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_bootstrap() -> anyhow::Result<()> {
    let tor_provider = build_bundled_legacy_tor_provider("test_legacy_bootstrap")?;
    bootstrap_test(tor_provider, false)
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_pluggable_transport_bootstrap() -> anyhow::Result<()> {
    let tor_provider = build_bundled_pt_legacy_tor_provider("test_legacy_pluggable_transport_bootstrap")?;

    if let Some(tor_provider) = tor_provider {
        bootstrap_test(tor_provider, false)?
    }
    Ok(())
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_onion_service() -> anyhow::Result<()> {
    let server_provider = build_bundled_legacy_tor_provider(
        "test_legacy_onion_service_server")?;
    let client_provider = build_bundled_legacy_tor_provider(
        "test_legacy_onion_service_client")?;

    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_legacy_authenticated_onion_service() -> anyhow::Result<()> {
    let server_provider = build_bundled_legacy_tor_provider("test_legacy_authenticated_onion_service_server")?;
    let client_provider = build_bundled_legacy_tor_provider("test_legacy_authenticated_onion_service_client")?;

    authenticated_onion_service_test(server_provider, client_provider)
}

//
// System Legacy TorProvider tests
//


#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_system_legacy_onion_service() -> anyhow::Result<()> {
    let server_provider = build_system_legacy_tor_provider(
        "test_system_legacy_onion_service_server",
        9251u16,
        9250u16)?;

    let client_provider = build_system_legacy_tor_provider(
        "test_system_legacy_onion_service_client",
        9351u16,
        9350u16)?;

    basic_onion_service_test(server_provider.0, client_provider.0)?;

    Ok(())
}

#[test]
#[serial]
#[cfg(feature = "legacy-tor-provider")]
fn test_system_legacy_authenticated_onion_service() -> anyhow::Result<()> {
    let server_provider = build_system_legacy_tor_provider(
        "test_system_legacy_authenticated_onion_service_server",
        9251u16,
        9250u16)?;

    let client_provider = build_system_legacy_tor_provider(
        "test_system_legacy_authenticated_onion_service_client",
        9351u16,
        9350u16)?;

    authenticated_onion_service_test(server_provider.0, client_provider.0)?;

    Ok(())
}

//
// Arti-Client TorProvider tests
//

#[test]
#[serial]
#[cfg(feature = "arti-client-tor-provider")]
fn test_arti_client_bootstrap() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let tor_provider = build_arti_client_tor_provider(runtime, "test_arti_client_bootstrap")?;
    bootstrap_test(tor_provider, false)
}

#[test]
#[cfg(feature = "arti-client-tor-provider")]
fn test_arti_client_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let server_provider = build_arti_client_tor_provider(runtime.clone(), "test_arti_basic_onion_service_server")?;
    let client_provider = build_arti_client_tor_provider(runtime.clone(), "test_arti_basic_onion_service_client")?;

    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(feature = "arti-client-tor-provider")]
fn test_arti_client_authenticated_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let server_provider = build_arti_client_tor_provider(runtime.clone(), "test_arti_authenticated_onion_service_server")?;
    let client_provider = build_arti_client_tor_provider(runtime.clone(), "test_arti_authenticated_onion_service_client")?;

    authenticated_onion_service_test(server_provider, client_provider)
}

//
// Arti TorProvider tests
//

#[test]
#[serial]
#[cfg(feature = "arti-tor-provider")]
fn test_arti_bootstrap() -> anyhow::Result<()> {
    let tor_provider = build_arti_tor_provider("test_arti_bootstrap")?;
    bootstrap_test(tor_provider, false)
}

//

//
// Mixed Arti/Legacy TorProvider tests
//

#[test]
#[serial]
#[cfg(all(feature = "arti-client-tor-provider", feature = "legacy-tor-provider"))]
fn test_mixed_arti_client_legacy_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let server_provider = build_arti_client_tor_provider(runtime, "test_mixed_arti_client_legacy_onion_service_server")?;
    let client_provider = build_bundled_legacy_tor_provider("test_mixed_arti_client_legacy_onion_service_client")?;

    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(all(feature = "arti-client-tor-provider", feature = "legacy-tor-provider"))]
fn test_mixed_legacy_arti_client_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let server_provider = build_bundled_legacy_tor_provider("test_mixed_legacy_arti_client_onion_service_server")?;
    let client_provider = build_arti_client_tor_provider(runtime, "test_mixed_legacy_arti_client_onion_service_client")?;

    basic_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(all(feature = "arti-client-tor-provider", feature = "legacy-tor-provider"))]
fn test_mixed_arti_client_legacy_authenticated_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let server_provider = build_arti_client_tor_provider(runtime, "test_mixed_arti_client_legacy_authenticated_onion_service_server")?;
    let client_provider = build_bundled_legacy_tor_provider("test_mixed_arti_client_legacy_authenticated_onion_service_client")?;

    authenticated_onion_service_test(server_provider, client_provider)
}

#[test]
#[serial]
#[cfg(all(feature = "arti-client-tor-provider", feature = "legacy-tor-provider"))]
fn test_mixed_legacy_arti_client_authenticated_onion_service() -> anyhow::Result<()> {
    let runtime: Arc<runtime::Runtime> = Arc::new(runtime::Runtime::new().unwrap());

    let server_provider = build_arti_client_tor_provider(runtime, "test_mixed_legacy_arti_client_authenticated_onion_service_server")?;
    let client_provider = build_bundled_legacy_tor_provider("test_mixed_legacy_arti_client_authenticated_onion_service_client")?;

    authenticated_onion_service_test(server_provider, client_provider)
}
