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

use crate::{NoiseError, NoiseSession};
use futures::Poll;
use log::{debug, trace};
use std::{fmt, io};
use tokio_io::{AsyncRead, AsyncWrite};

const MAX_NOISE_PKG_LEN: usize = 65535;
const MAX_WRITE_BUF_LEN: usize = 16384;
const TOTAL_BUFFER_LEN: usize = 2 * MAX_NOISE_PKG_LEN + 3 * MAX_WRITE_BUF_LEN;

/// A single `Buffer` contains multiple non-overlapping byte buffers.
struct Buffer {
    read: Vec<u8>,
    read_crypto: Vec<u8>,
    write: Vec<u8>,
    write_crypto: Vec<u8>,
}

impl Buffer {
    fn new() -> Buffer {
        Buffer {
            read: Vec::new(),
            read_crypto: Vec::new(),
            write: Vec::new(),
            write_crypto: Vec::new(),
        }
    }
}

/// A type used during the handshake phase, exchanging key material with the remote.
pub(super) struct Handshake<T>(NoiseOutput<T>);

impl<T> Handshake<T> {
    pub(super) fn new(io: T, session: NoiseSession) -> Self {
        Handshake(NoiseOutput::new(io, session))
    }
}

impl<T: AsyncRead + AsyncWrite> Handshake<T> {
    /// Send handshake message to remote.
    pub(super) fn send(&mut self) -> Poll<(), io::Error> {
        Ok(self.0.poll_write(&[])?.map(|_| ()))
    }

    /// Flush handshake message to remote.
    pub(super) fn flush(&mut self) -> Poll<(), io::Error> {
        self.0.poll_flush()
    }

    /// Receive handshake message from remote.
    pub(super) fn receive(&mut self) -> Poll<(), io::Error> {
        Ok(self.0.poll_read(&mut [])?.map(|_| ()))
    }

    /// Finish the handshake.
    ///
    /// This turns the noise session into transport mode and returns the remote's static
    /// public key as well as the established session for further communication.
    pub(super) fn finish(self) -> Result<([u8; 32], NoiseOutput<T>), NoiseError> {
        let p = self.0.session.get_remote_static()?;
        Ok((p, self.0))
    }
}

/// A noise session to a remote.
///
/// `T` is the type of the underlying I/O resource.
pub struct NoiseOutput<T> {
    io: T,
    session: NoiseSession,
    buffer: Buffer,
    read_state: ReadState,
    write_state: WriteState,
}

impl<T> fmt::Debug for NoiseOutput<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NoiseOutput")
            .field("read_state", &self.read_state)
            .field("write_state", &self.write_state)
            .finish()
    }
}

impl<T> NoiseOutput<T> {
    fn new(io: T, session: NoiseSession) -> Self {
        NoiseOutput {
            io,
            session,
            buffer: Buffer::new(),
            read_state: ReadState::Init,
            write_state: WriteState::Init,
        }
    }
}

/// The various states of reading a noise session transitions through.
#[derive(Debug)]
enum ReadState {
    /// initial state
    Init,
    /// read frame length
    ReadLen { buf: [u8; 2], off: usize },
    /// read encrypted frame data
    ReadData { len: usize, off: usize },
    /// copy decrypted frame data
    CopyData { len: usize, off: usize },
    /// end of file has been reached (terminal state)
    /// The associated result signals if the EOF was unexpected or not.
    Eof(Result<(), ()>),
    /// decryption error (terminal state)
    DecErr,
}

/// The various states of writing a noise session transitions through.
#[derive(Debug)]
enum WriteState {
    /// initial state
    Init,
    /// accumulate write data
    BufferData { off: usize },
    /// write frame length
    WriteLen {
        len: usize,
        buf: [u8; 2],
        off: usize,
    },
    /// write out encrypted data
    WriteData { len: usize, off: usize },
    /// end of file has been reached (terminal state)
    Eof,
    /// encryption error (terminal state)
    EncErr,
}

impl<T: io::Read> io::Read for NoiseOutput<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            trace!("read state: {:?}", self.read_state);
            match self.read_state {
                ReadState::Init => {
                    self.read_state = ReadState::ReadLen {
                        buf: [0, 0],
                        off: 0,
                    };
                }
                ReadState::ReadLen { mut buf, mut off } => {
                    let n = match read_frame_len(&mut self.io, &mut buf, &mut off) {
                        Ok(Some(n)) => n,
                        Ok(None) => {
                            trace!("read: eof");
                            self.read_state = ReadState::Eof(Ok(()));
                            return Ok(0);
                        }
                        Err(e) => {
                            if e.kind() == io::ErrorKind::WouldBlock {
                                // Preserve read state
                                self.read_state = ReadState::ReadLen { buf, off };
                            }
                            return Err(e);
                        }
                    };
                    trace!("read: next frame len = {}", n);
                    if n == 0 {
                        trace!("read: empty frame");
                        self.read_state = ReadState::Init;
                        continue;
                    }
                    self.read_state = ReadState::ReadData {
                        len: usize::from(n),
                        off: 0,
                    }
                }
                ReadState::ReadData { len, ref mut off } => {
                    let n = self.io.read(&mut self.buffer.read.as_mut_slice()[*off..len])?;
                    trace!("read: read {}/{} bytes", *off + n, len);
                    if n == 0 {
                        trace!("read: eof");
                        self.read_state = ReadState::Eof(Err(()));
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    *off += n;
                    if len == *off {
                        trace!("read: decrypting {} bytes", len);
                        if let Ok(plaintext) = self.session.read_message(Vec::from(&self.buffer.read[..len])) {
                            let n = plaintext.len();
                            self.buffer.read_crypto = plaintext;
                            trace!("read: payload len = {} bytes", n);
                            self.read_state = ReadState::CopyData { len: n, off: 0 }
                        } else {
                            debug!("decryption error");
                            self.read_state = ReadState::DecErr;
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                    }
                }
                ReadState::CopyData { len, ref mut off } => {
                    let n = std::cmp::min(len - *off, buf.len());
                    buf[..n].copy_from_slice(&self.buffer.read_crypto[*off..*off + n]);
                    trace!("read: copied {}/{} bytes", *off + n, len);
                    *off += n;
                    if len == *off {
                        self.read_state = ReadState::ReadLen {
                            buf: [0, 0],
                            off: 0,
                        };
                    }
                    return Ok(n);
                }
                ReadState::Eof(Ok(())) => {
                    trace!("read: eof");
                    return Ok(0);
                }
                ReadState::Eof(Err(())) => {
                    trace!("read: eof (unexpected)");
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                ReadState::DecErr => return Err(io::ErrorKind::InvalidData.into()),
            }
        }
    }
}

impl<T: io::Write> io::Write for NoiseOutput<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            trace!("write state: {:?}", self.write_state);
            match self.write_state {
                WriteState::Init => self.write_state = WriteState::BufferData { off: 0 },
                WriteState::BufferData { ref mut off } => {
                    let n = std::cmp::min(MAX_WRITE_BUF_LEN - *off, buf.len());
                    self.buffer.write =  Vec::from(&buf[..n]);
                    trace!("write: buffered {} bytes", *off + n);
                    *off += n;
                    if *off == MAX_WRITE_BUF_LEN {
                        trace!("write: encrypting {} bytes", *off);
                        if let Ok(ciphertext) = self.session.write_message(Vec::from(&self.buffer.write[..])) {
                            // clear self.buffer.write();
                            let n = ciphertext.len();
                            self.buffer.write_crypto = ciphertext;
                            trace!("write: cipher text len = {} bytes", n);
                            self.write_state = WriteState::WriteLen {
                                len: n,
                                buf: u16::to_be_bytes(n as u16),
                                off: 0,
                            }
                        } else {
                            debug!("encryption error");
                            self.write_state = WriteState::EncErr;
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                    }
                    return Ok(n);
                }
                WriteState::WriteLen {
                    len,
                    mut buf,
                    mut off,
                } => {
                    trace!("write: writing len ({}, {:?}, {}/2)", len, buf, off);
                    match write_frame_len(&mut self.io, &mut buf, &mut off) {
                        Err(e) => {
                            if e.kind() == io::ErrorKind::WouldBlock {
                                self.write_state = WriteState::WriteLen { len, buf, off };
                            }
                            return Err(e);
                        }
                        Ok(false) => {
                            trace!("write: eof");
                            self.write_state = WriteState::Eof;
                            return Err(io::ErrorKind::WriteZero.into());
                        }
                        Ok(true) => (),
                    }
                    self.write_state = WriteState::WriteData { len, off: 0 }
                }
                WriteState::WriteData { len, ref mut off } => {
                    let n = self.io.write(&self.buffer.write_crypto[*off..len])?;
                    trace!("write: wrote {}/{} bytes", *off + n, len);
                    if n == 0 {
                        trace!("write: eof");
                        self.write_state = WriteState::Eof;
                        return Err(io::ErrorKind::WriteZero.into());
                    }
                    *off += n;
                    if len == *off {
                        trace!("write: finished writing {} bytes", len);
                        self.write_state = WriteState::Init
                    }
                }
                WriteState::Eof => {
                    trace!("write: eof");
                    return Err(io::ErrorKind::WriteZero.into());
                }
                WriteState::EncErr => return Err(io::ErrorKind::InvalidData.into()),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        loop {
            match self.write_state {
                WriteState::Init => return Ok(()),
                WriteState::BufferData { off } => {
                    trace!("flush: encrypting {} bytes", off);
                    if let Ok(ciphertext) = self.session.write_message(Vec::from(&self.buffer.write[..off])) {
                        // clear self.buffer.write();
                        let n = ciphertext.len();
                        self.buffer.write_crypto = ciphertext;
                        trace!("flush: cipher text len = {} bytes", n);
                        self.write_state = WriteState::WriteLen {
                            len: n,
                            buf: u16::to_be_bytes(n as u16),
                            off: 0,
                        }
                    } else {
                        debug!("encryption error");
                        self.write_state = WriteState::EncErr;
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                }
                WriteState::WriteLen {
                    len,
                    mut buf,
                    mut off,
                } => {
                    trace!("flush: writing len ({}, {:?}, {}/2)", len, buf, off);
                    match write_frame_len(&mut self.io, &mut buf, &mut off) {
                        Ok(true) => (),
                        Ok(false) => {
                            trace!("write: eof");
                            self.write_state = WriteState::Eof;
                            return Err(io::ErrorKind::WriteZero.into());
                        }
                        Err(e) => {
                            if e.kind() == io::ErrorKind::WouldBlock {
                                // Preserve write state
                                self.write_state = WriteState::WriteLen { len, buf, off };
                            }
                            return Err(e);
                        }
                    }
                    self.write_state = WriteState::WriteData { len, off: 0 }
                }
                WriteState::WriteData { len, ref mut off } => {
                    let n = self.io.write(&self.buffer.write_crypto[*off..len])?;
                    trace!("flush: wrote {}/{} bytes", *off + n, len);
                    if n == 0 {
                        trace!("flush: eof");
                        self.write_state = WriteState::Eof;
                        return Err(io::ErrorKind::WriteZero.into());
                    }
                    *off += n;
                    if len == *off {
                        trace!("flush: finished writing {} bytes", len);
                        self.write_state = WriteState::Init;
                        return Ok(());
                    }
                }
                WriteState::Eof => {
                    trace!("flush: eof");
                    return Err(io::ErrorKind::WriteZero.into());
                }
                WriteState::EncErr => return Err(io::ErrorKind::InvalidData.into()),
            }
        }
    }
}

impl<T: AsyncRead> AsyncRead for NoiseOutput<T> {}

impl<T: AsyncWrite> AsyncWrite for NoiseOutput<T> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
}

/// Read 2 bytes as frame length from the given source into the given buffer.
///
/// Panics if `off >= 2`.
///
/// When [`io::ErrorKind::WouldBlock`] is returned, the given buffer and offset
/// may have been updated (i.e. a byte may have been read) and must be preserved
/// for the next invocation.
fn read_frame_len<R: io::Read>(
    io: &mut R,
    buf: &mut [u8; 2],
    off: &mut usize,
) -> io::Result<Option<u16>> {
    loop {
        let n = io.read(&mut buf[*off..])?;
        if n == 0 {
            return Ok(None);
        }
        *off += n;
        if *off == 2 {
            return Ok(Some(u16::from_be_bytes(*buf)));
        }
    }
}

/// Write 2 bytes as frame length from the given buffer into the given sink.
///
/// Panics if `off >= 2`.
///
/// When [`io::ErrorKind::WouldBlock`] is returned, the given offset
/// may have been updated (i.e. a byte may have been written) and must
/// be preserved for the next invocation.
fn write_frame_len<W: io::Write>(io: &mut W, buf: &[u8; 2], off: &mut usize) -> io::Result<bool> {
    loop {
        let n = io.write(&buf[*off..])?;
        if n == 0 {
            return Ok(false);
        }
        *off += n;
        if *off == 2 {
            return Ok(true);
        }
    }
}
