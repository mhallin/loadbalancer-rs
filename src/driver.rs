use std::io::{Result, Error, ErrorKind};
use std::rc::Rc;

use mio;
use mio::{Token, Handler, EventSet, PollOpt};
use mio::tcp::{TcpStream, TcpListener};

use frontend::Frontend;
use connection::{TokenType, ListenerToken, IncomingToken, OutgoingToken, Connection};
use driver_state::{DriverState, Listener};

type EventLoop = mio::EventLoop<Driver>;

pub struct Driver {
    state: DriverState,
}

pub enum DriverMessage {
    Shutdown,
}

impl Driver {
    pub fn new(state: DriverState) -> Driver {
        Driver {
            state: state,
        }
    }

    pub fn register(&mut self, event_loop: &mut EventLoop, frontend: Rc<Frontend>) -> Result<()> {
        for listen_addr in frontend.listen_addrs() {
            let listener = try!(TcpListener::bind(&listen_addr));
            let token = try!(self.state.listeners
                                 .insert(Listener {
                                     listener: listener,
                                     frontend: frontend.clone(),
                                 })
                                 .map_err(|_| {
                                     Error::new(ErrorKind::Other, "Listener buffer full")
                                 }));

            try!(event_loop.register_opt(&self.state.listeners.get(token).unwrap().listener,
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

        if let Some(listener) = self.state.listeners.get(token) {
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

            let outgoing_token = self.state.outgoing_connections
                                     .insert(None)
                                     .expect("Outgoing buffer full");

            let incoming_token = self.state.incoming_connections
                                     .insert_with(|incoming_token| {
                                         Connection::new(incoming,
                                                         incoming_token,
                                                         outgoing,
                                                         outgoing_token)
                                     })
                                     .expect("Incoming buffer full");

            self.state.outgoing_connections[outgoing_token] = Some(incoming_token);

            let connection = self.state.incoming_connections.get(incoming_token).unwrap();

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

        if let Some(mut connection) = self.state.incoming_connections.get_mut(token) {
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
        if let Some(&Some(incoming_token)) = self.state.outgoing_connections.get(token) {
            let mut remove = false;

            if let Some(mut connection) = self.state.incoming_connections.get_mut(incoming_token) {
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
        let connection = self.state.incoming_connections
                             .remove(token)
                             .expect("Can't remove already removed incoming connection");
        self.state.outgoing_connections
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

#[cfg(test)]
mod test {
    use super::{EventLoop, Driver, DriverMessage};

    use std::thread;
    use std::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};
    use std::net::{TcpStream, TcpListener, SocketAddr};
    use std::str::FromStr;
    use std::io::{Write, BufReader, BufRead};
    use std::time::Duration;

    use env_logger;

    use frontend::Frontend;
    use backend::Backend;
    use driver_state::DriverState;

    static PORT_NUMBER: AtomicUsize = ATOMIC_USIZE_INIT;

    fn next_port() -> u16 {
        let first_port = option_env!("TEST_BASE_PORT")
                             .map_or(32328, |v| v.parse::<usize>().unwrap());
        PORT_NUMBER.compare_and_swap(0, first_port, Ordering::SeqCst);

        PORT_NUMBER.fetch_add(1, Ordering::SeqCst) as u16
    }

    fn listen_addr() -> SocketAddr {
        let addr = format!("127.0.0.1:{}", next_port());
        FromStr::from_str(&addr).unwrap()
    }

    #[test]
    fn start_stop_driver() {
        let mut event_loop = EventLoop::new().unwrap();
        let sender = event_loop.channel();

        let t = thread::spawn(move || {
            let mut driver = Driver::new(DriverState::new());
            event_loop.run(&mut driver).unwrap();
        });

        sender.send(DriverMessage::Shutdown).unwrap();
        t.join().unwrap();
    }

    #[test]
    fn single_backend() {
        env_logger::init().unwrap();

        let mut event_loop = EventLoop::new().unwrap();
        let sender = event_loop.channel();
        let frontend_addr = listen_addr();
        let backend_addr = listen_addr();

        let t1 = thread::spawn(move || {
            let mut driver = Driver::new(DriverState::new());
            let backend = Backend::new(vec![backend_addr]);
            let frontend = Frontend::new(frontend_addr, vec![backend]);

            driver.register(&mut event_loop, frontend).unwrap();
            debug!("Starting event loop");

            event_loop.run(&mut driver).unwrap();
        });

        let t2 = thread::spawn(move || {
            debug!("Starting backend listener");

            let listener = TcpListener::bind(backend_addr).unwrap();
            let (mut client, _) = listener.accept().unwrap();

            write!(client, "sent by backend\n").unwrap();
            client.flush().unwrap();

            debug!("Backend wrote data, waiting for data now");

            let mut reader = BufReader::new(client);
            let mut buffer = String::new();
            reader.read_line(&mut buffer).unwrap();

            debug!("Backend done");

            assert_eq!(buffer, "sent by frontend\n");
        });

        thread::sleep(Duration::from_millis(100));

        {
            debug!("Connecting to frontend...");
            let client = TcpStream::connect(frontend_addr).unwrap();
            debug!("Frontend connected, waiting for data...");

            let mut reader = BufReader::new(client);
            let mut buffer = String::new();
            reader.read_line(&mut buffer).unwrap();

            debug!("Frontend read data, sending response");

            assert_eq!(buffer, "sent by backend\n");

            write!(reader.get_mut(), "sent by frontend\n").unwrap();
            reader.get_mut().flush().unwrap();

            debug!("Frontend done");
        }

        sender.send(DriverMessage::Shutdown).unwrap();

        t1.join().unwrap();
        t2.join().unwrap();
    }
}
