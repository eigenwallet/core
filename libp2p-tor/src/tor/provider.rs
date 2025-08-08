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

//! The UnixStream implementation is backported and open-coded from <https://github.com/libp2p/rust-libp2p/pull/6056>
//! It should be removed when libp2p is updated

use futures::{AsyncRead, AsyncWrite};
use tor_interface::tor_provider::{OnionAddr, OnionStream, TargetAddr, TcpOrUnixOnionStream, TcpOrUnixStream};
use libp2p::tcp::tokio::TcpStream;
#[cfg(unix)]
// use libp2p::unix_stream::tokio::UnixStream;
use tokio::net::UnixStream;

#[derive(Debug)]
enum TcpOrUnix {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

#[derive(Debug)]
pub struct OnionStreamStream {
    pub local_addr: Option<OnionAddr>,
    pub peer_addr: Option<TargetAddr>,
    stream: TcpOrUnix,
}

impl From<(tokio::net::TcpStream, Option<OnionAddr>)> for OnionStreamStream {
    fn from((stream, local_addr): (tokio::net::TcpStream, Option<OnionAddr>)) -> Self {
        let stream = TcpOrUnix::Tcp(TcpStream(stream));
        Self { local_addr, peer_addr: None, stream }
    }
}

impl OnionStreamStream {
    pub fn from_onion_stream(inner: TcpOrUnixOnionStream) -> std::io::Result<Self> {
        let local_addr = inner.local_addr();
        let peer_addr = inner.peer_addr();
        inner.set_nonblocking(true)?;
        let stream = match inner.into() {
            TcpOrUnixStream::Tcp(sock) => TcpOrUnix::Tcp(TcpStream(tokio::net::TcpStream::from_std(sock)?)),
            #[cfg(unix)]
            TcpOrUnixStream::Unix(sock) => TcpOrUnix::Unix(UnixStream::from_std(sock.into())?),
        };
        Ok(Self { local_addr, peer_addr, stream })
    }
}

impl AsyncRead for OnionStreamStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut self.stream {
            TcpOrUnix::Tcp(sock) => AsyncRead::poll_read(std::pin::Pin::new(sock), cx, buf),
            #[cfg(unix)]
            // TcpOrUnix::Unix(sock) => AsyncRead::poll_read(std::pin::Pin::new(&mut sock), cx, buf),
            TcpOrUnix::Unix(sock) => {
                let mut read_buf = tokio::io::ReadBuf::new(buf);
                futures::ready!(tokio::io::AsyncRead::poll_read(std::pin::Pin::new(sock), cx, &mut read_buf))?;
                std::task::Poll::Ready(Ok(read_buf.filled().len()))
            }
        }
    }

    fn poll_read_vectored(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &mut [std::io::IoSliceMut<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut self.stream {
            TcpOrUnix::Tcp(ref mut sock) => AsyncRead::poll_read_vectored(std::pin::Pin::new(sock), cx, bufs),
            #[cfg(unix)]
            // TcpOrUnix::Unix(ref mut sock) => AsyncRead::poll_read_vectored(std::pin::Pin::new(sock), cx, bufs),
            TcpOrUnix::Unix(_) => {
                // From default impl
                for b in bufs {
                    if !b.is_empty() {
                        return self.poll_read(cx, b);
                    }
                }

                self.poll_read(cx, &mut [])
            }
        }
    }
}

impl AsyncWrite for OnionStreamStream {
    #[inline]
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut self.stream {
            TcpOrUnix::Tcp(ref mut sock) => AsyncWrite::poll_write(std::pin::Pin::new(sock), cx, buf),
            #[cfg(unix)]
            // TcpOrUnix::Unix(sock) => AsyncWrite::poll_write(std::pin::Pin::new(sock), cx, buf),
            TcpOrUnix::Unix(ref mut sock) => tokio::io::AsyncWrite::poll_write(std::pin::Pin::new(sock), cx, buf)
        }
    }

    #[inline]
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.stream {
            TcpOrUnix::Tcp(ref mut sock) => AsyncWrite::poll_flush(std::pin::Pin::new(sock), cx),
            #[cfg(unix)]
            // TcpOrUnix::Unix(sock) => AsyncWrite::poll_flush(std::pin::Pin::new(sock), cx),
            TcpOrUnix::Unix(ref mut sock) => tokio::io::AsyncWrite::poll_flush(std::pin::Pin::new(sock), cx)
        }
    }

    #[inline]
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.stream {
            TcpOrUnix::Tcp(ref mut sock) => AsyncWrite::poll_close(std::pin::Pin::new(sock), cx),
            #[cfg(unix)]
            // TcpOrUnix::Unix(sock) => AsyncWrite::poll_close(std::pin::Pin::new(sock), cx),
            TcpOrUnix::Unix(ref mut sock) => tokio::io::AsyncWrite::poll_shutdown(std::pin::Pin::new(sock), cx)
        }
    }

    #[inline]
    fn poll_write_vectored(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut self.stream {
            TcpOrUnix::Tcp(ref mut sock) => AsyncWrite::poll_write_vectored(std::pin::Pin::new(sock), cx, bufs),
            #[cfg(unix)]
            // TcpOrUnix::Unix(sock) => AsyncWrite::poll_write_vectored(std::pin::Pin::new(sock), cx, bufs),
            TcpOrUnix::Unix(ref mut sock) => tokio::io::AsyncWrite::poll_write_vectored(std::pin::Pin::new(sock), cx, bufs)
        }
    }
}
