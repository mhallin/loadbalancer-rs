use mio::{Token, EventSet, TryRead, TryWrite};
use mio::tcp::TcpStream;

use slab::Index;

#[derive(Debug, Copy, Clone)]
pub enum TokenType {
    Listener(ListenerToken),
    Incoming(IncomingToken),
    Outgoing(OutgoingToken),
}

#[derive(Debug, Copy, Clone)]
pub struct ListenerToken(pub usize);

#[derive(Debug, Copy, Clone)]
pub struct IncomingToken(pub usize);

#[derive(Debug, Copy, Clone)]
pub struct OutgoingToken(pub usize);

#[derive(Debug)]
pub struct Connection {
    incoming_state: EventSet,
    incoming_stream: TcpStream,
    incoming_token: IncomingToken,

    outgoing_state: EventSet,
    outgoing_stream: TcpStream,
    outgoing_token: OutgoingToken,
}

impl Connection {
    pub fn new(incoming_stream: TcpStream,
               incoming_token: IncomingToken,
               outgoing_stream: TcpStream,
               outgoing_token: OutgoingToken)
               -> Connection {
        Connection {
            incoming_state: EventSet::none(),
            incoming_stream: incoming_stream,
            incoming_token: incoming_token,

            outgoing_state: EventSet::none(),
            outgoing_stream: outgoing_stream,
            outgoing_token: outgoing_token,
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

    pub fn tick(&mut self) {
        if self.outgoing_state.is_writable() && self.incoming_state.is_readable() {
            transfer(&mut self.incoming_stream, &mut self.outgoing_stream)
        }

        if self.incoming_state.is_writable() && self.outgoing_state.is_readable() {
            transfer(&mut self.outgoing_stream, &mut self.incoming_stream)
        }
    }
}

fn transfer(src: &mut TcpStream, dest: &mut TcpStream) {
    while transfer_chunk(src, dest) {
    }
}

fn transfer_chunk(src: &mut TcpStream, dest: &mut TcpStream) -> bool {
    let mut buf = [0u8; 4096];

    match src.try_read(&mut buf) {
        Ok(Some(n_read)) => {
            trace!("Read {} bytes", n_read);

            match dest.try_write(&buf[0..n_read]) {
                Ok(Some(n_written)) => {
                    trace!("Wrote {} bytes", n_written);

                    if n_written == buf.len() {
                        return true;
                    }
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
