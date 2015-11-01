use mio::{Sender, Token, EventSet};
use mio::tcp::TcpStream;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct BackendId(pub u64);

#[derive(Debug)]
pub enum BackendMessage {
    AcceptConnection(Token, TcpStream, FrontendSender),
    ConnectionEvent(Token, BackendId, EventSet),
}

#[derive(Debug)]
pub enum FrontendMessage {
    SocketCreated {
        frontend_token: Token,
        backend_token: Token,
        backend_id: BackendId,
    },
    SocketClosed(Token, TcpStream),
}

pub type BackendSender = Sender<BackendMessage>;
pub type FrontendSender = Sender<FrontendMessage>;
