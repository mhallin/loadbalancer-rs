use mio::{Token, EventSet, TryRead, TryWrite};
use mio::tcp::TcpStream;

use slab::Index;

#[derive(Debug, Copy, Clone)]
pub enum TokenType {
    Listener(ListenerToken),
    Incoming(IncomingToken),
    Outgoing(OutgoingToken),
}

#[derive(PartialEq, Eq, Hash, Debug, Copy, Clone)]
pub struct ListenerToken(pub usize);

#[derive(PartialEq, Eq, Hash, Debug, Copy, Clone)]
pub struct IncomingToken(pub usize);

#[derive(PartialEq, Eq, Hash, Debug, Copy, Clone)]
pub struct OutgoingToken(pub usize);

type BufferArray = [u8; 4096];

pub struct Connection {
    incoming_state: EventSet,
    incoming_stream: TcpStream,
    incoming_buffer: BufferArray,
    incoming_buffer_size: usize,
    incoming_total_transfer: usize,

    outgoing_state: EventSet,
    outgoing_stream: TcpStream,
    outgoing_token: OutgoingToken,
    outgoing_buffer: BufferArray,
    outgoing_buffer_size: usize,
    outgoing_total_transfer: usize,
}

impl Connection {
    pub fn new(incoming_stream: TcpStream,
               outgoing_stream: TcpStream,
               outgoing_token: OutgoingToken)
               -> Connection {
        Connection {
            incoming_state: EventSet::none(),
            incoming_stream: incoming_stream,
            incoming_buffer: [0; 4096],
            incoming_buffer_size: 4096,
            incoming_total_transfer: 0,

            outgoing_state: EventSet::none(),
            outgoing_stream: outgoing_stream,
            outgoing_token: outgoing_token,
            outgoing_buffer: [0; 4096],
            outgoing_buffer_size: 4096,
            outgoing_total_transfer: 0,
        }
    }

    pub fn incoming_ready(&mut self, events: EventSet) {
        self.incoming_state.insert(events);
    }

    pub fn outgoing_ready(&mut self, events: EventSet) {
        self.outgoing_state.insert(events);
    }

    pub fn is_outgoing_closed(&self) -> bool {
        self.outgoing_state.is_error() || self.outgoing_state.is_hup()
    }

    pub fn is_incoming_closed(&self) -> bool {
        self.incoming_state.is_error() || self.incoming_state.is_hup()
    }

    pub fn incoming_stream<'a>(&'a self) -> &'a TcpStream {
        &self.incoming_stream
    }

    pub fn outgoing_stream<'a>(&'a self) -> &'a TcpStream {
        &self.outgoing_stream
    }

    pub fn outgoing_token(&self) -> OutgoingToken {
        self.outgoing_token
    }

    pub fn tick(&mut self) -> bool {
        trace!("Connection in state [incoming {:?}] [outgoing {:?}]",
               self.incoming_state,
               self.outgoing_state);

        let mut data_sent = false;
        let mut could_send = false;

        if self.incoming_buffer.len() != self.incoming_buffer_size &&
           self.outgoing_state.is_writable() {
            could_send = true;
            data_sent |= flush_buffer(&mut self.incoming_buffer,
                                      &mut self.incoming_buffer_size,
                                      &mut self.outgoing_stream,
                                      &mut self.outgoing_total_transfer);
            self.outgoing_state.remove(EventSet::writable());
        }

        if self.outgoing_buffer.len() != self.outgoing_buffer_size &&
           self.incoming_state.is_writable() {
            could_send = true;
            data_sent |= flush_buffer(&mut self.outgoing_buffer,
                                      &mut self.outgoing_buffer_size,
                                      &mut self.incoming_stream,
                                      &mut self.incoming_total_transfer);
            self.incoming_state.remove(EventSet::writable());
        }

        if self.outgoing_state.is_writable() && self.incoming_state.is_readable() {
            could_send = true;
            data_sent |= transfer(&mut self.incoming_buffer,
                                  &mut self.incoming_buffer_size,
                                  &mut self.incoming_stream,
                                  &mut self.outgoing_stream,
                                  &mut self.outgoing_total_transfer);
            self.incoming_state.remove(EventSet::readable());
            self.outgoing_state.remove(EventSet::writable());
        }

        if self.incoming_state.is_writable() && self.outgoing_state.is_readable() {
            could_send = true;
            data_sent |= transfer(&mut self.outgoing_buffer,
                                  &mut self.outgoing_buffer_size,
                                  &mut self.outgoing_stream,
                                  &mut self.incoming_stream,
                                  &mut self.incoming_total_transfer);
            self.incoming_state.remove(EventSet::writable());
            self.outgoing_state.remove(EventSet::readable());
        }

        !could_send || data_sent
    }
}

fn flush_buffer(buf: &BufferArray,
                buf_size: &mut usize,
                dest: &mut TcpStream,
                total: &mut usize)
                -> bool {
    let start_index = *buf_size;
    let bytes_to_write = buf.len() - start_index;

    trace!("Will flush {} bytes", bytes_to_write);

    match dest.try_write(&buf[start_index..]) {
        Ok(Some(n_written)) => {
            *total += n_written;
            trace!("Flushed {} bytes, total {}", n_written, *total);

            assert!(bytes_to_write == n_written, "Must flush entire buffer");

            *buf_size = buf.len();

            return n_written > 0;
        }
        Ok(None) => {
            trace!("Writing would block");
        }
        Err(e) => {
            error!("Writing caused error: {}", e);
        }
    }

    return false;
}

fn transfer(buf: &mut BufferArray,
            buf_size: &mut usize,
            src: &mut TcpStream,
            dest: &mut TcpStream,
            total: &mut usize)
            -> bool {
    match src.try_read(buf) {
        Ok(Some(n_read)) => {
            trace!("Read {} bytes", n_read);

            match dest.try_write(&buf[0..n_read]) {
                Ok(Some(n_written)) => {
                    *total += n_written;
                    trace!("Wrote {} bytes, total {}", n_written, *total);

                    if n_written < n_read {
                        *buf_size = buf.len() - (n_read - n_written);
                    } else {
                        *buf_size = buf.len();
                    }

                    return n_written > 0;
                }
                Ok(None) => {
                    trace!("Writing would block");
                }
                Err(e) => {
                    error!("Writing caused error: {}", e);
                }
            }
        }
        Ok(None) => {
            trace!("Reading would block");
        }
        Err(e) => {
            error!("Reading caused error: {}", e);
        }
    }

    return false;
}

impl TokenType {
    pub fn from_raw_token(t: Token) -> TokenType {
        let i = t.as_usize();

        match i & 3 {
            0 => TokenType::Listener(ListenerToken(i >> 2)),
            1 => TokenType::Incoming(IncomingToken(i >> 2)),
            2 => TokenType::Outgoing(OutgoingToken(i >> 2)),
            _ => unreachable!(),
        }
    }
}


impl ListenerToken {
    pub fn as_raw_token(self) -> Token {
        Token(self.0 << 2)
    }
}

impl IncomingToken {
    pub fn as_raw_token(self) -> Token {
        Token((self.0 << 2) + 1)
    }
}

impl OutgoingToken {
    pub fn as_raw_token(self) -> Token {
        Token((self.0 << 2) + 2)
    }
}

impl Index for ListenerToken {
    fn from_usize(i: usize) -> ListenerToken {
        ListenerToken(i)
    }

    fn as_usize(&self) -> usize {
        self.0
    }
}

impl Index for IncomingToken {
    fn from_usize(i: usize) -> IncomingToken {
        IncomingToken(i)
    }

    fn as_usize(&self) -> usize {
        self.0
    }
}

impl Index for OutgoingToken {
    fn from_usize(i: usize) -> OutgoingToken {
        OutgoingToken(i)
    }

    fn as_usize(&self) -> usize {
        self.0
    }
}
