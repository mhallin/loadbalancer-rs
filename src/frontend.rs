use std::io::Result;
use std::net::SocketAddr;
use std::thread;

use mio;
use mio::{Token, Handler, EventSet, PollOpt};
use mio::tcp::TcpListener;
use mio::util::Slab;

use messages::{BackendId, BackendSender, BackendMessage, FrontendMessage};

const SERVER: Token = Token(1);

type EventLoop = mio::EventLoop<FrontendHandler>;

pub struct Frontend {
    event_loop: EventLoop,
    handler: FrontendHandler,
}

struct FrontendHandler {
    listener: TcpListener,
    backend_sender: BackendSender,
    connections: Slab<FrontendConnection>,
}

#[derive(Debug)]
struct FrontendConnection {
    state: EventSet,
    backend_ref: Option<(Token, BackendId)>,
}

impl Frontend {
    pub fn new(listen_addr: &SocketAddr, backend_sender: BackendSender) -> Result<Frontend> {
        let server = try!(TcpListener::bind(listen_addr));

        Ok(Frontend {
            event_loop: try!(EventLoop::new()),
            handler: FrontendHandler {
                listener: server,
                backend_sender: backend_sender,
                connections: Slab::new_starting_at(Token(2), 4096),
            },
        })
    }

    pub fn run(mut self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            self.event_loop
                .register_opt(&self.handler.listener,
                              SERVER,
                              EventSet::readable(),
                              PollOpt::edge())
                .unwrap();

            info!("Starting frontend event loop");

            self.event_loop.run(&mut self.handler).unwrap();
        })
    }
}

impl FrontendHandler {
    fn accept_socket(&mut self, event_loop: &mut EventLoop) {
        debug!("Accepting socket connection");

        match self.listener.accept() {
            Ok(Some(s)) => {
                let token = self.connections
                                .insert(FrontendConnection {
                                    state: EventSet::none(),
                                    backend_ref: None,
                                })
                                .unwrap();

                event_loop.register_opt(&s, token, EventSet::all(), PollOpt::edge())
                          .unwrap();

                self.backend_sender
                    .send(BackendMessage::AcceptConnection(token, s, event_loop.channel()))
                    .unwrap();
            }
            Ok(None) => {
                info!("Unknown accept error");
            }
            Err(e) => {
                error!("Accept error: {}", e);
            }
        }
    }
}

impl Handler for FrontendHandler {
    type Timeout = ();
    type Message = FrontendMessage;

    fn ready(&mut self, event_loop: &mut EventLoop, token: Token, events: EventSet) {
        if events.is_readable() && token == SERVER {
            self.accept_socket(event_loop);
        } else if token.as_usize() > 1 {
            trace!("Frontend event {:?} on token {:?}", events, token);

            if let Some((backend_token, backend_id)) = self.connections[token].backend_ref {
                self.backend_sender
                    .send(BackendMessage::ConnectionEvent(backend_token, backend_id, events))
                    .unwrap();
            }

            self.connections[token].state.insert(events);
        }
    }

    fn notify(&mut self, event_loop: &mut EventLoop, msg: FrontendMessage) {
        match msg {
            FrontendMessage::SocketCreated { frontend_token, backend_token, backend_id } => {
                self.connections[frontend_token].backend_ref = Some((backend_token, backend_id));

                trace!("Frontend connection {:?} linked with backend",
                       backend_token);

                let state = self.connections[frontend_token].state;

                if state != EventSet::none() {
                    self.backend_sender
                        .send(BackendMessage::ConnectionEvent(backend_token, backend_id, state))
                        .unwrap();
                }
            }

            FrontendMessage::SocketClosed(frontend_token, frontend_stream) => {
                trace!("Frontend deregistering and removing {:?}", frontend_token);
                event_loop.deregister(&frontend_stream).unwrap();
                self.connections.remove(frontend_token).unwrap();
            }
        }
    }
}
