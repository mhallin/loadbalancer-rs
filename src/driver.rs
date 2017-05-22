use std::collections::HashSet;

use mio::{Poll, PollOpt, Events, Ready};
use mio::tcp::TcpStream;

use slab::Slab;

// use config::RootConfig;
use connection::{TokenType, ListenerToken, IncomingToken, OutgoingToken, Connection};
use driver_state::DriverState;

pub struct Driver {
    to_reregister: HashSet<IncomingToken>,
    incoming_connections: Slab<Connection, IncomingToken>,
    outgoing_connections_token: Slab<Option<IncomingToken>, OutgoingToken>,
    state: DriverState,
}

// pub enum DriverMessage {
//     Shutdown,
//     Reconfigure(RootConfig),
// }

impl Driver {
    pub fn new(state: DriverState) -> Driver {
        Driver {
            to_reregister: HashSet::new(),
            incoming_connections: Slab::new_starting_at(IncomingToken(1),
                                                        state.config.buffers.connections),
            outgoing_connections_token: Slab::new_starting_at(OutgoingToken(1),
                                                              state.config.buffers.connections),
            state: state,
        }
    }

    fn listener_ready(&mut self, poll: &mut Poll, token: ListenerToken, event: Ready) {
        assert!(event.is_readable());

        if let Some(listener) = self.state.listeners.get(token) {
            info!("Accepting connection");

            let incoming = match listener.listener.accept() {
                Ok((sock, _)) => sock,
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

            let outgoing_token = self.outgoing_connections_token
                .insert(None)
                .expect("Outgoing buffer full");

            let incoming_token = self.incoming_connections
                .insert(Connection::new(incoming, outgoing, outgoing_token))
                .map_err(|_| "Incoming buffer full")
                .unwrap();

            self.outgoing_connections_token[outgoing_token] = Some(incoming_token);

            let connection = self.incoming_connections.get(incoming_token).unwrap();

            info!("IncomingToken {:?}", incoming_token.as_raw_token());
            info!("OutgoingToken {:?}", outgoing_token.as_raw_token());
            poll.register(connection.incoming_stream(),
                          incoming_token.as_raw_token(),
                          Ready::readable() | Ready::writable(),
                          PollOpt::edge() | PollOpt::oneshot())
                .unwrap();
            poll.register(connection.outgoing_stream(),
                          outgoing_token.as_raw_token(),
                          Ready::readable() | Ready::writable(),
                          PollOpt::edge() | PollOpt::oneshot())
                .unwrap();

            poll.reregister(&listener.listener,
                            token.as_raw_token(),
                            Ready::readable() | Ready::writable(),
                            PollOpt::edge() | PollOpt::oneshot())
                .unwrap();
        } else {
            error!("Listener event on unknown token {:?}", token);
        }
    }

    fn incoming_ready(&mut self, token: IncomingToken, ready: Ready) {
        let mut remove = false;

        if let Some(mut connection) = self.incoming_connections.get_mut(token) {
            info!("in incoming ready {:?} {:?}", token, ready.is_readable());
            connection.incoming_ready(ready);
            let data_sent = connection.tick();
            if !data_sent && (connection.is_incoming_closed() || connection.is_outgoing_closed()) {
                remove = true;
            } else if data_sent || ready.is_readable() {
                self.to_reregister.insert(token);
            }
        } else {
            warn!("Could not find incoming connection for {:?}", token);
        }

        if remove {
            self.remove_connection(token);
        }
    }

    fn outgoing_ready(&mut self, token: OutgoingToken, ready: Ready) {
        if let Some(&Some(incoming_token)) = self.outgoing_connections_token.get(token) {
            let mut remove = false;

            if let Some(mut connection) = self.incoming_connections.get_mut(incoming_token) {
                connection.outgoing_ready(ready);
                let data_sent = connection.tick();

                if !data_sent && connection.is_outgoing_closed() {
                    remove = true;
                } else if data_sent {
                    //info!("to_reregister token {:?} from outgoing_ready", token);
                    self.to_reregister.insert(incoming_token);
                }
            } else {
                warn!("Could not find corresponding incoming connection for {:?} -> {:?}",
                      token,
                      incoming_token);
            }

            if remove {
                debug!("Clearing connection from {:?} -> {:?}",
                       token,
                       incoming_token);
                self.outgoing_connections_token[token] = None
            }
        } else {
            warn!("Could not find outgoing connection for {:?}", token);
        }
    }

    fn remove_connection(&mut self, token: IncomingToken) {
        debug!("Removing connection on incoming token {:?}", token);
        let connection = self.incoming_connections
            .remove(token)
            .expect("Can't remove already removed incoming connection");
        self.outgoing_connections_token
            .remove(connection.outgoing_token())
            .expect("Can't remove already removed outgoing connection");
    }

    fn tick(&mut self, poll: &mut Poll) {
        for token in self.to_reregister.iter() {
            if let Some(connection) = self.incoming_connections.get(*token) {
                poll.reregister(connection.incoming_stream(),
                                token.as_raw_token(),
                                Ready::readable() | Ready::writable(),
                                PollOpt::edge() | PollOpt::oneshot())
                    .unwrap();

                poll.reregister(connection.outgoing_stream(),
                                connection.outgoing_token().as_raw_token(),
                                Ready::readable() | Ready::writable(),
                                PollOpt::edge() | PollOpt::oneshot())
                    .unwrap();
            }
        }

        self.to_reregister.clear();

        for token in self.state.listeners_to_remove.iter() {
            info!("Removing listener on token {:?}", token);

            let listener = self.state.listeners.remove(*token).unwrap();

            poll.deregister(&listener.listener).unwrap();
            drop(listener);
        }

        self.state.listeners_to_remove.clear();
    }


    pub fn run(&mut self, poll: &mut Poll, events: &mut Events) {
        loop {
            poll.poll(events, None).unwrap();

            for event in events.iter() {
                match TokenType::from_raw_token(event.token()) {
                    TokenType::Listener(token) => {
                        println!("listener token");
                        self.listener_ready(poll, token, event.readiness())
                    }
                    TokenType::Incoming(token) => self.incoming_ready(token, event.readiness()),
                    TokenType::Outgoing(token) => self.outgoing_ready(token, event.readiness()),
                }
            }
            self.tick(poll);
        }
    }
}

//#[cfg(test)]
//mod test {
//    use super::{EventLoop, Driver, DriverMessage};
//
//    use std::thread;
//    use std::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};
//    use std::net::{TcpStream, TcpListener, SocketAddr};
//    use std::str::FromStr;
//    use std::io::{Write, BufReader, BufRead};
//    use std::time::Duration;
//    use std::collections::HashMap;
//    use std::default::Default;
//
//    use env_logger;
//
//    use config::RootConfig;
//    use driver_state::DriverState;
//
//    static PORT_NUMBER: AtomicUsize = ATOMIC_USIZE_INIT;
//
//    fn next_port() -> u16 {
//        let first_port =
//            option_env!("TEST_BASE_PORT").map_or(32328, |v| v.parse::<usize>().unwrap());
//        PORT_NUMBER.compare_and_swap(0, first_port, Ordering::SeqCst);
//
//        PORT_NUMBER.fetch_add(1, Ordering::SeqCst) as u16
//    }
//
//    #[test]
//    fn start_stop_driver() {
//        env_logger::init().unwrap_or(());
//
//        let mut event_loop = EventLoop::new().unwrap();
//        let sender = event_loop.channel();
//
//        let t = thread::spawn(move || {
//                                  let mut driver =
//                                      Driver::new(DriverState::new(&Default::default()));
//                                  event_loop.run(&mut driver).unwrap();
//                              });
//
//        sender.send(DriverMessage::Shutdown).unwrap();
//        t.join().unwrap();
//    }
//}
//
//    #[test]
//    fn single_backend() {
//        env_logger::init().unwrap_or(());
//
//        let mut event_loop = EventLoop::new().unwrap();
//        let sender = event_loop.channel();
//
//        let config = RootConfig::from_str(&format!("[frontends.in]
//listen_addr = \
//                                                    \"127.0.0.1:{}\"
//backend = \"out\"
//
//\
//                                                    [backends.out]
//target_addrs = \
//                                                    [\"127.0.0.1:{}\"]
//
//[buffers]
//connections = \
//                                                    4096
//listeners = 128
//",
//                                                   next_port(),
//                                                   next_port()))
//                .unwrap();
//
//        let backend_addr: SocketAddr = FromStr::from_str(&config.backends["out"].target_addrs[0])
//            .unwrap();
//        let frontend_addr: SocketAddr = FromStr::from_str(&config.frontends["in"].listen_addr)
//            .unwrap();
//
//        let t1 = thread::spawn(move || {
//            let mut driver_state = DriverState::new(&Default::default());
//            driver_state
//                .reconfigure(&mut event_loop, &config)
//                .unwrap();
//            let mut driver = Driver::new(driver_state);
//
//            debug!("Starting event loop");
//
//            event_loop.run(&mut driver).unwrap();
//        });
//
//        let t2 = thread::spawn(move || {
//            debug!("Starting backend listener");
//
//            let listener = TcpListener::bind(backend_addr).unwrap();
//            let (mut client, _) = listener.accept().unwrap();
//
//            write!(client, "sent by backend\n").unwrap();
//            client.flush().unwrap();
//
//            debug!("Backend wrote data, waiting for data now");
//
//            let mut reader = BufReader::new(client);
//            let mut buffer = String::new();
//            reader.read_line(&mut buffer).unwrap();
//
//            debug!("Backend done");
//
//            assert_eq!(buffer, "sent by frontend\n");
//        });
//
//        thread::sleep(Duration::from_millis(100));
//
//        {
//            debug!("Connecting to frontend...");
//            let client = TcpStream::connect(frontend_addr).unwrap();
//            debug!("Frontend connected, waiting for data...");
//
//            let mut reader = BufReader::new(client);
//            let mut buffer = String::new();
//            reader.read_line(&mut buffer).unwrap();
//
//            debug!("Frontend read data, sending response");
//
//            assert_eq!(buffer, "sent by backend\n");
//
//            write!(reader.get_mut(), "sent by frontend\n").unwrap();
//            reader.get_mut().flush().unwrap();
//
//            debug!("Frontend done");
//        }
//
//        sender.send(DriverMessage::Shutdown).unwrap();
//
//        t1.join().unwrap();
//        t2.join().unwrap();
//    }
//
//    #[test]
//    fn test_reconfigure_remove_listen() {
//        env_logger::init().unwrap_or(());
//
//        let mut event_loop = EventLoop::new().unwrap();
//        let sender = event_loop.channel();
//
//        let config = RootConfig::from_str(&format!("[frontends.in]
//listen_addr = \
//                                                    \"127.0.0.1:{}\"
//backend = \"out\"
//
//\
//                                                    [backends.out]
//target_addrs = \
//                                                    [\"127.0.0.1:{}\"]
//
//[buffers]
//connections = \
//                                                    4096
//listeners = 128
//",
//                                                   next_port(),
//                                                   next_port()))
//                .unwrap();
//
//        let frontend_addr: SocketAddr = FromStr::from_str(&config.frontends["in"].listen_addr)
//            .unwrap();
//
//        let t1 = thread::spawn(move || {
//            let mut driver_state = DriverState::new(&Default::default());
//            driver_state
//                .reconfigure(&mut event_loop, &config)
//                .unwrap();
//            let mut driver = Driver::new(driver_state);
//
//            debug!("Starting event loop");
//
//            event_loop.run(&mut driver).unwrap();
//        });
//
//        thread::sleep(Duration::from_millis(100));
//
//        {
//            let client = TcpStream::connect(frontend_addr);
//
//            assert!(client.is_ok());
//        }
//
//        sender
//            .send(DriverMessage::Reconfigure(RootConfig {
//                                                 frontends: HashMap::new(),
//                                                 backends: HashMap::new(),
//                                                 ..Default::default()
//                                             }))
//            .expect("Should be able to send reconfigure message");
//
//        thread::sleep(Duration::from_millis(100));
//
//        {
//            debug!("Trying to connect, expecting refused connection");
//            let client = TcpStream::connect(frontend_addr);
//
//            assert!(client.is_err());
//        }
//
//        sender
//            .send(DriverMessage::Shutdown)
//            .expect("Should be able to send shutdown message");
//
//        t1.join()
//            .expect("Event loop thread should have exited cleanly");
//    }
//}
