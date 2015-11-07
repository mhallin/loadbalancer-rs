use std::rc::Rc;

use mio::tcp::TcpListener;

use slab::Slab;

use frontend::Frontend;
use connection::{ListenerToken, IncomingToken, OutgoingToken, Connection};

pub struct Listener {
    pub listener: TcpListener,
    pub frontend: Rc<Frontend>,
}

pub struct DriverState {
    pub incoming_connections: Slab<Connection, IncomingToken>,
    pub outgoing_connections: Slab<Option<IncomingToken>, OutgoingToken>,
    pub listeners: Slab<Listener, ListenerToken>,
}

impl DriverState {
    pub fn new() -> DriverState {
        DriverState {
            incoming_connections: Slab::new_starting_at(IncomingToken(1), 4096),
            outgoing_connections: Slab::new_starting_at(OutgoingToken(1), 4096),
            listeners: Slab::new_starting_at(ListenerToken(1), 128),
        }
    }
}
