use mio::{Token, EventSet, TryRead, TryWrite};
use mio::tcp::TcpStream;

use messages::{BackendId, FrontendSender};

#[derive(Debug)]
pub struct Connection {
    frontend_state: EventSet,
    frontend_stream: TcpStream,
    frontend_token: Token,
    frontend_reply: FrontendSender,

    backend_state: EventSet,
    backend_stream: TcpStream,
    backend_token: Token,
    backend_id: BackendId,
}

impl Connection {
    pub fn new(frontend_token: Token,
               frontend_stream: TcpStream,
               frontend_reply: FrontendSender,
               backend_token: Token,
               backend_stream: TcpStream,
               backend_id: BackendId)
               -> Connection {
        Connection {
            frontend_state: EventSet::none(),
            frontend_stream: frontend_stream,
            frontend_token: frontend_token,
            frontend_reply: frontend_reply,

            backend_state: EventSet::none(),
            backend_stream: backend_stream,
            backend_token: backend_token,
            backend_id: backend_id,
        }
    }

    pub fn frontend_ready(&mut self, events: EventSet) {
        self.frontend_state.insert(events);
    }

    pub fn backend_ready(&mut self, events: EventSet) {
        self.backend_state.insert(events);
    }

    pub fn is_backend_closed(&self) -> bool {
        self.backend_state.is_error() || self.backend_state.is_hup()
    }

    pub fn is_frontend_closed(&self) -> bool {
        self.frontend_state.is_error() || self.frontend_state.is_hup()
    }

    pub fn tick(&mut self) {
        if self.backend_state.is_writable() && self.frontend_state.is_readable() {
            transfer(&mut self.frontend_stream, &mut self.backend_stream)
        }

        if self.frontend_state.is_writable() && self.backend_state.is_readable() {
            transfer(&mut self.backend_stream, &mut self.frontend_stream)
        }
    }

    pub fn backend_stream<'a>(&'a self) -> &'a TcpStream {
        &self.backend_stream
    }

    pub fn backend_id(&self) -> BackendId {
        self.backend_id
    }

    pub fn frontend_token(&self) -> Token {
        self.frontend_token
    }

    pub fn frontend_stream(self) -> TcpStream {
        self.frontend_stream
    }

    pub fn frontend_reply<'a>(&'a self) -> &'a FrontendSender {
        &self.frontend_reply
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
