use std::io::{Result, Error, ErrorKind};
use std::rc::Rc;

use mio;
use mio::{Token, Handler, EventSet, PollOpt};
use mio::tcp::{TcpStream, TcpListener};

use slab::Slab;

use frontend::Frontend;
use connection::{TokenType, ListenerToken, IncomingToken, OutgoingToken, Connection};

type EventLoop = mio::EventLoop<Driver>;

pub struct Driver {
    incoming_connections: Slab<Connection, IncomingToken>,
    outgoing_connections: Slab<Option<IncomingToken>, OutgoingToken>,
    listeners: Slab<Listener, ListenerToken>,
}

pub enum DriverMessage {
    Shutdown,
}

struct Listener {
    listener: TcpListener,
    frontend: Rc<Frontend>,
}

impl Driver {
    pub fn new() -> Driver {
        Driver {
            incoming_connections: Slab::new_starting_at(IncomingToken(1), 4096),
            outgoing_connections: Slab::new_starting_at(OutgoingToken(1), 4096),
            listeners: Slab::new_starting_at(ListenerToken(1), 128),
        }
    }

    pub fn register(&mut self, event_loop: &mut EventLoop, frontend: Rc<Frontend>) -> Result<()> {
        for listen_addr in frontend.listen_addrs() {
            let listener = try!(TcpListener::bind(&listen_addr));
            let token = try!(self.listeners
                                 .insert(Listener {
                                     listener: listener,
                                     frontend: frontend.clone(),
                                 })
                                 .map_err(|_| {
                                     Error::new(ErrorKind::Other, "Listener buffer full")
                                 }));

            try!(event_loop.register_opt(&self.listeners.get(token).unwrap().listener,
                                         token.as_raw_token(),
                                         EventSet::readable(),
                                         PollOpt::edge()));

            debug!("Added listener or {:?}", listen_addr);
        }

        Ok(())
    }

    fn listener_ready(&mut self,
                      event_loop: &mut EventLoop,
                      token: ListenerToken,
                      events: EventSet) {
        assert!(events.is_readable());

        if let Some(listener) = self.listeners.get(token) {
            info!("Accepting connection");

            let incoming = match listener.listener.accept() {
                Ok(Some(client)) => client,
                Ok(None) => {
                    warn!("Accept would block");
                    return;
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                    return;
                }
            };

            let backend = listener.frontend.decide_backend();
            let target = backend.borrow_mut().decide_target();

            let outgoing = match TcpStream::connect(&target) {
                Ok(client) => client,
                Err(e) => {
                    error!("Connect error: {}", e);
                    return;
                }
            };

            let outgoing_token = self.outgoing_connections
                                     .insert(None)
                                     .expect("Outgoing buffer full");

            let incoming_token = self.incoming_connections
                                     .insert_with(|incoming_token| {
                                         Connection::new(incoming,
                                                         incoming_token,
                                                         outgoing,
                                                         outgoing_token)
                                     })
                                     .expect("Incoming buffer full");

            self.outgoing_connections[outgoing_token] = Some(incoming_token);

            let connection = self.incoming_connections.get(incoming_token).unwrap();

            event_loop.register_opt(connection.incoming_stream(),
                                    incoming_token.as_raw_token(),
                                    EventSet::all(),
                                    PollOpt::edge())
                      .unwrap();
            event_loop.register_opt(connection.outgoing_stream(),
                                    outgoing_token.as_raw_token(),
                                    EventSet::all(),
                                    PollOpt::edge())
                      .unwrap();
        } else {
            error!("Listener event on unknown token");
        }
    }

    fn incoming_ready(&mut self,
                      event_loop: &mut EventLoop,
                      token: IncomingToken,
                      events: EventSet) {
        let mut remove = false;

        if let Some(mut connection) = self.incoming_connections.get_mut(token) {
            connection.incoming_ready(events);
            connection.tick();

            if connection.is_incoming_closed() {
                remove = true;
            }
        }

        if remove {
            self.remove_connection(event_loop, token);
        }
    }

    fn outgoing_ready(&mut self,
                      event_loop: &mut EventLoop,
                      token: OutgoingToken,
                      events: EventSet) {
        if let Some(&Some(incoming_token)) = self.outgoing_connections.get(token) {
            let mut remove = false;

            if let Some(mut connection) = self.incoming_connections.get_mut(incoming_token) {
                connection.outgoing_ready(events);
                connection.tick();

                if connection.is_outgoing_closed() {
                    remove = true;
                }
            }

            if remove {
                self.remove_connection(event_loop, incoming_token);
            }
        }
    }

    fn remove_connection(&mut self, event_loop: &mut EventLoop, token: IncomingToken) {
        let connection = self.incoming_connections
                             .remove(token)
                             .expect("Can't remove already removed incoming connection");
        self.outgoing_connections
            .remove(connection.outgoing_token())
            .expect("Can't remove already removed outgoing connection");

        event_loop.deregister(connection.incoming_stream())
                  .expect("Can't deregister already deregistered incoming stream");
        event_loop.deregister(connection.outgoing_stream())
                  .expect("Can't deregister already deregistered outgoing stream");
    }
}

impl Handler for Driver {
    type Timeout = ();
    type Message = DriverMessage;

    fn ready(&mut self, event_loop: &mut EventLoop, token: Token, events: EventSet) {
        if token == Token(0) {
            trace!("Event on token zero");
            return;
        }

        trace!("Event on token {:?}", token);

        match TokenType::from_raw_token(token) {
            TokenType::Listener(token) => self.listener_ready(event_loop, token, events),
            TokenType::Incoming(token) => self.incoming_ready(event_loop, token, events),
            TokenType::Outgoing(token) => self.outgoing_ready(event_loop, token, events),
        }
    }

    fn notify(&mut self, event_loop: &mut EventLoop, msg: DriverMessage) {
        match msg {
            DriverMessage::Shutdown => event_loop.shutdown(),
        }
    }
}
