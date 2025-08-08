// Copyright 2022 Hannes Furmans
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![warn(clippy::pedantic)]
#![deny(unsafe_code)]
//! Tor based transport for libp2p. Connect through the Tor network to TCP listeners.
//!
//! # ⚠️ Misuse warning ⚠️ - read carefully before using
//! Although the sound of "Tor" might convey a sense of security it is *very* easy to misuse this
//! crate and leaking private information while using. Study libp2p carefully and try to make sure
//! you fully understand it's current limits regarding privacy. I.e. using identify might already
//! render this transport obsolete.
//!
//! This transport explicitly **doesn't** provide any enhanced privacy if it's just used like a regular transport.
//! Use with caution and at your own risk. **Don't** just blindly advertise Tor without fully understanding what you
//! are dealing with.
//!
//! ## Runtime
//!
//! This crate uses tokio with rustls for its runtime and TLS implementation.
//! No other combinations are supported.
//!
//! ## Examples
//! ```no_run
//! use libp2p::core::Transport;
//! # async fn test_func() -> Result<(), Box<dyn std::error::Error>> {
//! let address = "/dns/www.torproject.org/tcp/1000".parse()?;
//! let mut transport = libp2p_tor::TorTransport::bootstrapped().await?;
//! // we have achieved tor connection
//! let _conn = transport.dial(address)?.await?;
//! # Ok(())
//! # }
//! # tokio_test::block_on(test_func());
//! ```
//!
//! ```no_run
//! use libp2p::core::Transport;
//! use std::sync::{Arc, Mutex};
//! use libp2p_tor::tor_interface::tor_provider::TorProvider;
//! # async fn test_func() -> Result<(), Box<dyn std::error::Error>> {
//! let address = "/dns/www.torproject.org/tcp/1000".parse()?;
//! let mut provider = libp2p_tor::tor_interface::legacy_tor_client::LegacyTorClient::new(
//!     libp2p_tor::tor_interface::legacy_tor_client::LegacyTorClientConfig::system_from_environment().unwrap())?;
//! provider.bootstrap()?;
//! let mut transport = libp2p_tor::TorInterfaceTransport::from_provider(Default::default(), Arc::new(Mutex::new(provider)), None)?;
//! // we have achieved tor connection
//! let _conn = transport.dial(address)?.await?;
//! # Ok(())
//! # }
//! # tokio_test::block_on(test_func());
//! ```

mod arti;
pub use arti::*;

#[cfg(feature="tor-interface")]
mod tor;
#[cfg(feature="tor-interface")]
pub use tor::*;
