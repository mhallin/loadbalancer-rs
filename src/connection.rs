//use std::io;
use mio::{Token, Ready};
use mio::unix::UnixReady;
use mio::tcp::TcpStream;
use std::io::prelude::*;
use std::io::ErrorKind;
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

#[derive(Copy, Clone)]
pub enum EndPointType {
    Front = 0,
    Back = 1,
}

pub struct EndPoint {
    state: Ready,
    stream: TcpStream,
    buffer: BufferArray,
    buffer_index: usize,
}

impl EndPoint {
    pub fn new(tcp_stream: TcpStream) -> EndPoint {
        EndPoint {
            state: Ready::empty(),
            stream: tcp_stream,
            buffer: [0; 4096],
            buffer_index: 0,
        }
    }

    pub fn absorb(buf: &mut BufferArray, index: &mut usize, src: &mut TcpStream) -> usize {
        match src.read(buf.split_at_mut(*index).1) {
            Ok(n_read) => {
                info!("### Read {} bytes", n_read);
                *index += n_read;
                return n_read;
            }
            Err(e) => {
                if e.kind() == ErrorKind::WouldBlock {
                    //                    info!("WouldBlock when read");
                    return 0;
                }
                error!("Reading caused error: {}", e);
            }
        }
        return 0;
    }

    pub fn pipe(buf: &mut BufferArray, size: usize, dest: &mut TcpStream) -> usize {
        info!("in pipe size is {}", size);
        match dest.write(buf.split_at(size).0) {
            Ok(n_written) => {
                info!("### Write {} bytes", n_written);
                if n_written < size {
                    error!("do not support shorten writeen");
                }
                return n_written;
            }
            Err(e) => {
                if e.kind() == ErrorKind::WouldBlock {
                    // info!("WouldBlock when read");
                    return 0;
                }

                error!("Reading caused error: {}", e);
                return 0;
            }
        }
    }
}

pub struct Connection {
    points: [EndPoint; 2],
    backend_token: OutgoingToken,
}

impl Connection {
    pub fn new(incoming_stream: TcpStream,
               outgoing_stream: TcpStream,
               outgoing_token: OutgoingToken)
               -> Connection {
        Connection {
            points: [EndPoint::new(incoming_stream),
                     EndPoint::new(outgoing_stream)],
            backend_token: outgoing_token,
        }
    }

    pub fn incoming_ready(&mut self, events: Ready) {
        self.points[EndPointType::Front as usize]
            .state
            .insert(events);
    }

    pub fn outgoing_ready(&mut self, events: Ready) {
        self.points[EndPointType::Back as usize]
            .state
            .insert(events);
    }

    pub fn is_outgoing_closed(&self) -> bool {
        let unix_ready = UnixReady::from(self.points[EndPointType::Back as usize].state);

        unix_ready.is_error() || unix_ready.is_hup()
    }

    pub fn is_incoming_closed(&self) -> bool {
        let unix_ready = UnixReady::from(self.points[EndPointType::Front as usize].state);

        unix_ready.is_error() || unix_ready.is_hup()
    }

    pub fn incoming_stream<'a>(&'a self) -> &'a TcpStream {
        &self.points[EndPointType::Front as usize].stream
    }

    pub fn outgoing_stream<'a>(&'a self) -> &'a TcpStream {
        &self.points[EndPointType::Back as usize].stream
    }

    pub fn outgoing_token(&self) -> OutgoingToken {
        self.backend_token
    }

    pub fn transfer(&mut self, src: EndPointType, dest: EndPointType) -> usize {
        let mut count = 0;
        if self.points[src as usize].buffer_index > 0 &&
           self.points[dest as usize].state.is_writable() {
            count = EndPoint::pipe(&mut self.points[src as usize].buffer,
                                   self.points[src as usize].buffer_index,
                                   &mut self.points[dest as usize].stream);
            self.points[src as usize].buffer_index = 0;
        }
        count
    }
    pub fn tick(&mut self) -> bool {
        //        trace!("Connection in state [incoming {:?}] [outgoing {:?}]",
        //               self.incoming_state,
        //               self.outgoing_state);

        let mut sended = false;
        for point in self.points.iter_mut() {
            if point.state.is_readable() {
                info!("point state is readable");
                EndPoint::absorb(&mut point.buffer,
                                 &mut point.buffer_index,
                                 &mut point.stream);
                point.state.remove(Ready::readable());
            }
        }

        sended |= self.transfer(EndPointType::Back, EndPointType::Front) > 0;
        sended |= self.transfer(EndPointType::Front, EndPointType::Back) > 0;
        sended
    }
}

impl TokenType {
    pub fn from_raw_token(t: Token) -> TokenType {
        let i = usize::from(t);

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
