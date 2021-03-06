// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! [Noise protocol framework][noise] support for libp2p.
//!
//! This crate provides `libp2p_core::InboundUpgrade` and `libp2p_core::OutboundUpgrade`
//! implementations for various noise handshake patterns (currently IK, IX, and XX)
//! over a particular choice of DH key agreement (currently only X25519).
//!
//! All upgrades produce as output a pair, consisting of the remote's static public key
//! and a `NoiseOutput` which represents the established cryptographic session with the
//! remote, implementing `tokio_io::AsyncRead` and `tokio_io::AsyncWrite`.
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! use libp2p_core::Transport;
//! use libp2p_tcp::TcpConfig;
//! use libp2p_noise::{Keypair, X25519, NoiseConfig};
//!
//! # fn main() {
//! let keys = Keypair::<X25519>::new();
//! let transport = TcpConfig::new().with_upgrade(NoiseConfig::xx(keys));
//! // ...
//! # }
//! ```
//!
//! [noise]: http://noiseprotocol.org/

mod error;
mod io;

pub mod rt1;
pub mod rt15;

pub use error::NoiseError;
pub use io::NoiseOutput;
pub use noiseexplorer::{
    noisesession_ik, noisesession_ix, noisesession_xx,
    types::{Keypair, PublicKey},
};

use libp2p_core::{upgrade::Negotiated, InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use tokio_io::{AsyncRead, AsyncWrite};

pub enum NoiseSession {
    ik(noisesession_ik::NoiseSession),
    ix(noisesession_ix::NoiseSession),
    xx(noisesession_xx::NoiseSession),
}

impl NoiseSession {
    pub fn read_message(&mut self, mut transport_buffer: Vec<u8>) -> Result<Vec<u8>, NoiseError> {
        match self {
            NoiseSession::ik(a) => {
                if a.get_message_count() > 1 && a.is_transport() {
                    // transport
                    // read all of it then output before index transport_buffer.len()-16
                    a.recv_message(&mut transport_buffer[..])?;
                    let (plaintext, _) = transport_buffer.split_at(transport_buffer.len()-16);
                    Ok(Vec::from(&plaintext[..]))
                }
                else if a.get_message_count() == 0 {
                    // message a, mc = 0
                    // read all of it then output between index 80 and index transport_buffer.len()-16
                    a.recv_message(&mut transport_buffer[..])?;
                    let (plaintext, _) = &transport_buffer[80..].split_at(transport_buffer.len()-16);
                    Ok(Vec::from(&plaintext[..]))
                }
                else {
                    // message b, mc = 1
                    // read all of it then output between index 32 and index transport_buffer.len()-16
                    a.recv_message(&mut transport_buffer[..])?;
                    let (plaintext, _) = &transport_buffer[32..].split_at(transport_buffer.len()-16);
                    Ok(Vec::from(&plaintext[..]))
                }
            }
            NoiseSession::ix(a) => {
                if a.get_message_count() > 1 && a.is_transport() {
                    // transport
                    // read all of it then output before index transport_buffer.len()-16
                    a.recv_message(&mut transport_buffer[..])?;
                    let (plaintext, _) = transport_buffer.split_at(transport_buffer.len()-16);
                    Ok(Vec::from(&plaintext[..]))
                }
                else if a.get_message_count() == 0 {
                    // message a, mc = 0
                    // read all of it then output after 64
                    a.recv_message(&mut transport_buffer[..])?;
                    Ok(Vec::from(&transport_buffer[64..]))
                }
                else {
                    // message b, mc = 1
                    // read all of it then output between index 80 and index transport_buffer.len()-16
                    a.recv_message(&mut transport_buffer[..])?;
                    let (plaintext, _) = &transport_buffer[80..].split_at(transport_buffer.len()-16);
                    Ok(Vec::from(&plaintext[..]))
                }

            }
            NoiseSession::xx(a) => {
                if a.get_message_count() > 2 && a.is_transport() {
                    // transport
                    // read all of it then output before index transport_buffer.len()-16
                    a.recv_message(&mut transport_buffer[..])?;
                    let (plaintext, _) = transport_buffer.split_at(transport_buffer.len()-16);
                    Ok(Vec::from(&plaintext[..]))
                }
                else if a.get_message_count() == 0 {
                    // message a, mc = 0
                    // read all of it then output after 32
                    a.recv_message(&mut transport_buffer[..])?;
                    Ok(Vec::from(&transport_buffer[32..]))
                }
                else if a.get_message_count() == 1 {
                    // message b, mc = 1
                    // read all of it then output between index 80 and index transport_buffer.len()-16
                    a.recv_message(&mut transport_buffer[..])?;
                    let (plaintext, _) = &transport_buffer[80..].split_at(transport_buffer.len()-16);
                    Ok(Vec::from(&plaintext[..]))
                }
                else {
                    // message c, mc = 2
                    // read all of it then output between index 48 and index transport_buffer.len()-16
                    a.recv_message(&mut transport_buffer[..])?;
                    let (plaintext, _) = &transport_buffer[48..].split_at(transport_buffer.len()-16);
                    Ok(Vec::from(&plaintext[..]))
                }
            }
        }
    }
    pub fn write_message(&mut self, mut plaintext: Vec<u8>) -> Result<Vec<u8>, NoiseError> {
        match self {
            NoiseSession::ik(a) => {
                if a.get_message_count() > 1 && a.is_transport() {
                    // transport
                    plaintext.extend_from_slice(&mut [0u8; 16][..]);
                    a.send_message(&mut plaintext[..])?;
                    Ok(plaintext)
                }
                else if a.get_message_count() == 0 {
                    // message a, mc = 0
                    // append 80 empty bytes at start
                    // append plaintext
                    // append 16 at end
                    let mut output: Vec<u8> = Vec::from(&[0u8; 80][..]);
                    output.append(&mut plaintext);
                    output.extend_from_slice(&mut [0u8; 16][..]);
                    a.send_message(&mut output[..])?;
                    //destroy plaintext
                    Ok(output)
                }
                else {
                    // message b, mc = 1
                    // 32 empty bytes at start
                    // append plaintext
                    // append 16 at end
                    let mut output: Vec<u8> = Vec::from(&[0u8; 32][..]);
                    output.append(&mut plaintext);
                    output.extend_from_slice(&mut [0u8; 16][..]);
                    a.send_message(&mut output[..])?;
                    //destroy plaintext
                    Ok(output)
                }
            }
            NoiseSession::ix(a) => {
                if a.get_message_count() > 1 && a.is_transport() {
                    // transport
                    // append 16 at end
                    plaintext.extend_from_slice(&mut [0u8; 16][..]);
                    a.send_message(&mut plaintext[..])?;
                    Ok(plaintext)
                }
                else if a.get_message_count() == 0 {
                    // message a, mc = 0
                    // append 64 empty bytes at start
                    // append plaintext
                    let mut output: Vec<u8> = Vec::from(&[0u8; 64][..]);
                    output.append(&mut plaintext);
                    a.send_message(&mut output[..])?;
                    Ok(output)
                }
                else {
                    // message b, mc = 1
                    // 80 empty bytes at start
                    // append plaintext
                    // append 16 at end
                    let mut output: Vec<u8> = Vec::from(&[0u8; 80][..]);
                    output.append(&mut plaintext);
                    output.extend_from_slice(&mut [0u8; 16][..]);
                    a.send_message(&mut output[..])?;
                    Ok(output)
                }
            }
            NoiseSession::xx(a) => {
                if a.get_message_count() > 2 && a.is_transport() {
                    // transport
                    // append 16 at end
                    plaintext.extend_from_slice(&mut [0u8; 16][..]);
                    a.send_message(&mut plaintext[..])?;
                    Ok(plaintext)
                }
                else if a.get_message_count() == 0 {
                    // message a, mc = 0
                    // append 32 empty bytes at start
                    // append plaintext
                    let mut output: Vec<u8> = Vec::from(&[0u8; 32][..]);
                    output.append(&mut plaintext);
                    a.send_message(&mut output[..])?;
                    Ok(output)
                }
                else if a.get_message_count() == 1 {
                    // message b, mc = 1
                    // 80 empty bytes at start
                    // append plaintext
                    // append 16 at end
                    let mut output: Vec<u8> = Vec::from(&[0u8; 80][..]);
                    output.append(&mut plaintext);
                    output.extend_from_slice(&mut [0u8; 16][..]);
                    a.send_message(&mut output[..])?;
                    Ok(output)
                }
                else {
                    // message c, mc = 2
                    // 48 empty bytes at start
                    // append plaintext
                    // append 16 at end
                    let mut output: Vec<u8> = Vec::from(&[0u8; 48][..]);
                    output.append(&mut plaintext);
                    output.extend_from_slice(&mut [0u8; 16][..]);
                    a.send_message(&mut output[..])?;
                    Ok(output)
                }
            }
        }
    }
    fn get_remote_static(&self) -> Result<[u8; 32], NoiseError> {
        match self {
            NoiseSession::ik(a) => {
                if let Some(public_key) = a.get_remote_static_public_key() {
                    let a = public_key.as_bytes();
                    Ok(a)
                }
                else {
                    //todo incomplete handshake error
                    Err(NoiseError::InvalidKey)
                }
            }
            NoiseSession::ix(a) => {
                if let Some(public_key) = a.get_remote_static_public_key() {
                    let a = public_key.as_bytes();
                    Ok(a)
                }
                else {
                    //todo incomplete handshake error
                    Err(NoiseError::InvalidKey)
                }
            }
            NoiseSession::xx(a) => {
                if let Some(public_key) = a.get_remote_static_public_key() {
                    let a = public_key.as_bytes();
                    Ok(a)
                }
                else {
                    //todo incomplete handshake error
                    Err(NoiseError::InvalidKey)
                }
            }
        }
    }
}

// Handshake pattern IX /////////////////////////////////////////////////////
#[derive(Clone)]
pub struct IX(Keypair);

impl IX {
    /// Create a new `NoiseConfig` for the IX handshake pattern.
    pub fn new(k: Keypair) -> IX {
        IX(k)
    }
}

impl UpgradeInfo for IX {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ix/25519/chachapoly/blake2s/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for IX
where
    T: AsyncRead + AsyncWrite,
    IX: UpgradeInfo,
{
    type Output = ([u8; 32], NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = rt1::NoiseInboundFuture<Negotiated<T>>;

    fn upgrade_inbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let prologue = b"Noise_IX_25519_ChaChaPoly_Blake2s";
            let session = NoiseSession::ix(noisesession_ix::NoiseSession::init_session(
                false,
                prologue,
                self.0,
            ));
            return rt1::NoiseInboundFuture::new(socket, session);
    }
}

impl<T> OutboundUpgrade<T> for IX
where
    T: AsyncRead + AsyncWrite,
    IX: UpgradeInfo,
{
    type Output = ([u8; 32], NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = rt1::NoiseOutboundFuture<Negotiated<T>>;

    fn upgrade_outbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let prologue = b"Noise_IX_25519_ChaChaPoly_Blake2s";
            let session = NoiseSession::ix(noisesession_ix::NoiseSession::init_session(
                true,
                prologue,
                self.0
            ));
            return rt1::NoiseOutboundFuture::new(socket, session);
    }
}

// Handshake pattern XX /////////////////////////////////////////////////////
#[derive(Clone)]
pub struct XX(Keypair);

impl XX {
    /// Create a new configuration for the XX handshake pattern.
    pub fn new(k: Keypair) -> Self {
        XX(k)
    }
}

impl UpgradeInfo for XX {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/xx/25519/chachapoly/blake2s/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for XX
where
    T: AsyncRead + AsyncWrite,
    XX: UpgradeInfo,
{
    type Output = ([u8; 32], NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = rt15::NoiseInboundFuture<Negotiated<T>>;

    fn upgrade_inbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let prologue = b"Noise_IX_25519_ChaChaPoly_Blake2s";
            let session = NoiseSession::xx(noisesession_xx::NoiseSession::init_session(
                false,
                prologue,
                self.0
            ));
            return rt15::NoiseInboundFuture::new(socket, session);
    }
}

impl<T> OutboundUpgrade<T> for XX
where
    T: AsyncRead + AsyncWrite,
    XX: UpgradeInfo,
{
    type Output = ([u8; 32], NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = rt15::NoiseOutboundFuture<Negotiated<T>>;

    fn upgrade_outbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let prologue = b"Noise_XX_25519_ChaChaPoly_Blake2s";
            let session = NoiseSession::xx(noisesession_xx::NoiseSession::init_session(
                true,
                prologue,
                self.0,
            ));
            return rt15::NoiseOutboundFuture::new(socket, session);
    }
}

// Handshake pattern IK /////////////////////////////////////////////////////
#[derive(Clone)]
pub struct IK(Keypair, [u8; 32]);

impl IK {
    /// Create a new `NoiseConfig` for the IK handshake pattern (recipient side).
    pub fn new_listener(k: Keypair) -> IK {
        IK(k, PublicKey::empty().as_bytes())
    }
    /// Create a new `NoiseConfig` for the IK handshake pattern (initiator side).
    pub fn new_dialer(k: Keypair, remote: PublicKey) -> IK {
        IK(k, remote.as_bytes())
    }
}

impl UpgradeInfo for IK {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ik/25519/chachapoly/blake2s/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for IK
where
    T: AsyncRead + AsyncWrite,
    IK: UpgradeInfo,
{
    type Output = ([u8; 32], NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = rt1::NoiseInboundFuture<Negotiated<T>>;

    fn upgrade_inbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let prologue = b"Noise_IK_25519_ChaChaPoly_Blake2s";
            let session = NoiseSession::ik(noisesession_ik::NoiseSession::init_session(
                false,
                prologue,
                self.0,
                None,
            ));
            return rt1::NoiseInboundFuture::new(socket, session);
    }
}

impl<T> OutboundUpgrade<T> for IK
where
    T: AsyncRead + AsyncWrite,
    IK: UpgradeInfo,
{
    type Output = ([u8; 32], NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = rt1::NoiseOutboundFuture<Negotiated<T>>;

    fn upgrade_outbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let prologue = b"Noise_IK_25519_ChaChaPoly_Blake2s";
        let public_key = PublicKey::from_bytes(self.1).unwrap();
            let session = NoiseSession::ik(noisesession_ik::NoiseSession::init_session(
                true,
                prologue,
                self.0,
                Some(public_key),
            ));
            return rt1::NoiseOutboundFuture::new(socket, session);
    }
}
