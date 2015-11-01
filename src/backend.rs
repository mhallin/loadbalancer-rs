use std::thread;
use std::net::SocketAddr;
use std::io::Result;

use mio;
use mio::{Token, EventSet, Handler, PollOpt};
use mio::tcp::TcpStream;
use mio::util::Slab;

use connection::Connection;
use messages::{BackendId, BackendMessage, FrontendMessage};

pub type EventLoop = mio::EventLoop<BackendHandler>;
pub type Sender = mio::Sender<BackendMessage>;

pub struct Backend {
    event_loop: EventLoop,
    handler: BackendHandler,
}

pub struct BackendHandler {
    connections: Slab<Connection>,
    target_addr: SocketAddr,
    next_id: u64,
}

impl Backend {
    pub fn new(target_addr: SocketAddr) -> Result<Backend> {
        Ok(Backend {
            event_loop: try!(EventLoop::new()),
            handler: BackendHandler {
                connections: Slab::new_starting_at(Token(1), 4096),
                target_addr: target_addr,
                next_id: 0,
            },
        })
    }

    pub fn channel(&self) -> Sender {
        self.event_loop.channel()
    }

    pub fn run(mut self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            info!("Starting backend event loop");

            self.event_loop.run(&mut self.handler).unwrap();
        })
    }
}

impl BackendHandler {
    fn remove_connection(&mut self, event_loop: &mut EventLoop, token: Token) {
        trace!("Backend deregistering and removing {:?}", token);

        if let Some(connection) = self.connections.remove(token) {
            event_loop.deregister(connection.backend_stream()).unwrap();

            connection.frontend_reply()
                      .clone()
                      .send(FrontendMessage::SocketClosed(connection.frontend_token(),
                                                          connection.frontend_stream()))
                      .unwrap();
        }
    }
}

impl Handler for BackendHandler {
    type Timeout = ();
    type Message = BackendMessage;

    fn ready(&mut self, event_loop: &mut EventLoop, token: Token, events: EventSet) {
        trace!("Backend event {:?} on token {:?}", events, token);

        let mut remove = false;

        if token.as_usize() > 0 {
            let mut connection = &mut self.connections[token];

            connection.backend_ready(events);
            connection.tick();

            if connection.is_backend_closed() {
                remove = true;
            }
        }

        if remove {
            self.remove_connection(event_loop, token);
        }
    }

    fn notify(&mut self, event_loop: &mut EventLoop, msg: BackendMessage) {
        match msg {
            BackendMessage::AcceptConnection(frontend_token, frontend_stream, reply) => {
                let backend_stream = TcpStream::connect(&self.target_addr).unwrap();
                let cloned_reply = reply.clone();
                let backend_id = BackendId(self.next_id);
                self.next_id += 1;

                let backend_token = self.connections
                                        .insert_with(|backend_token| {
                                            Connection::new(frontend_token,
                                                            frontend_stream,
                                                            cloned_reply,
                                                            backend_token,
                                                            backend_stream,
                                                            backend_id)
                                        })
                                        .unwrap();

                event_loop.register_opt(self.connections[backend_token].backend_stream(),
                                        backend_token,
                                        EventSet::all(),
                                        PollOpt::edge())
                          .unwrap();

                reply.send(FrontendMessage::SocketCreated {
                         frontend_token: frontend_token,
                         backend_token: backend_token,
                         backend_id: backend_id,
                     })
                     .unwrap();
            }

            BackendMessage::ConnectionEvent(backend_token, backend_id, events) => {
                trace!("Backend received connection event on token {:?}",
                       backend_token);
                let mut remove = false;

                if let Some(mut connection) = self.connections.get_mut(backend_token) {
                    if connection.backend_id() != backend_id {
                        return;
                    }

                    connection.frontend_ready(events);
                    connection.tick();

                    if connection.is_frontend_closed() {
                        remove = true;
                    }
                }

                if remove {
                    self.remove_connection(event_loop, backend_token);
                }
            }
        }
    }
}
