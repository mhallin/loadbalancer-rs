use std::rc::Rc;
use std::cell::RefCell;
use std::net::{ToSocketAddrs, SocketAddr};
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::io::{ErrorKind, Result as IOResult, Error as IOError};

use mio::{PollOpt, EventSet, Handler, EventLoop};
use mio::tcp::TcpListener;

use slab::Slab;

use backend::Backend;
use frontend::Frontend;
use connection::{ListenerToken, IncomingToken, OutgoingToken, Connection};
use config::{RootConfig, BackendConfig, FrontendConfig, BufferConfig};

pub struct Listener {
    pub listener: TcpListener,
    pub listen_addr: SocketAddr,
    pub frontend: Rc<Frontend>,
    pub token: ListenerToken,
}

pub struct DriverState {
    pub incoming_connections: Slab<Connection, IncomingToken>,
    pub outgoing_connections: Slab<Option<IncomingToken>, OutgoingToken>,
    pub listeners: Slab<Listener, ListenerToken>,
}

impl DriverState {
    pub fn new(buffers: &BufferConfig) -> DriverState {
        DriverState {
            incoming_connections: Slab::new_starting_at(IncomingToken(1), buffers.connections),
            outgoing_connections: Slab::new_starting_at(OutgoingToken(1), buffers.connections),
            listeners: Slab::new_starting_at(ListenerToken(1), buffers.listeners),
        }
    }

    pub fn reconfigure<T>(&mut self,
                          event_loop: &mut EventLoop<T>,
                          config: RootConfig)
                          -> IOResult<()>
        where T: Handler
    {
        trace!("Reconfiguring driver state: {:#?}", config);

        let mut backends = HashMap::new();
        let mut frontends = HashMap::new();

        for (name, config) in config.backends.iter() {
            backends.insert(name, try!(make_backend(config)));
        }

        for (name, config) in config.frontends.iter() {
            frontends.insert(name, try!(make_frontend(config, &backends)));
        }

        let mut listeners_to_add: HashMap<SocketAddr, Rc<Frontend>> = HashMap::new();
        let mut listeners_to_remove: HashSet<ListenerToken> = HashSet::new();

        {
            let mut listeners_by_addr = self.listeners
                                            .iter_mut()
                                            .map(|l| (l.listen_addr, l))
                                            .collect::<HashMap<SocketAddr, &mut Listener>>();

            for (_, frontend) in frontends {
                for listen_addr in frontend.listen_addrs() {
                    match listeners_by_addr.entry(listen_addr) {
                        Occupied(mut e) => {
                            e.get_mut().frontend = frontend.clone();
                            e.remove();
                        }
                        Vacant(_) => {
                            listeners_to_add.insert(listen_addr, frontend.clone());
                        }
                    }
                }
            }

            for (_, listener) in listeners_by_addr.into_iter() {
                listeners_to_remove.insert(listener.token);
            }
        }

        for (addr, frontend) in listeners_to_add.into_iter() {
            let tcp_listener = try!(TcpListener::bind(&addr));
            let token = try!(self.listeners
                                 .insert_with(|token| {
                                     Listener {
                                         listener: tcp_listener,
                                         listen_addr: addr,
                                         token: token,
                                         frontend: frontend,
                                     }
                                 })
                                 .ok_or(IOError::new(ErrorKind::Other, "Listener buffer full")));
            let listener = &self.listeners[token];

            try!(event_loop.register_opt(&listener.listener,
                                         listener.token.as_raw_token(),
                                         EventSet::readable(),
                                         PollOpt::edge()));
        }

        for token in listeners_to_remove.into_iter() {
            let listener = try!(self.listeners
                                    .remove(token)
                                    .ok_or(IOError::new(ErrorKind::Other,
                                                        "Could not remove listener")));

            try!(event_loop.deregister(&listener.listener));
        }

        Ok(())
    }
}

fn resolve_name(s: &str) -> IOResult<SocketAddr> {
    let addrs: Vec<SocketAddr> = try!(s.to_socket_addrs()).collect();

    assert_eq!(addrs.len(), 1);

    Ok(addrs[0])
}

fn make_backend(config: &BackendConfig) -> IOResult<Rc<RefCell<Backend>>> {
    let target_addrs = config.target_addrs
                             .iter()
                             .flat_map(|s| {
                                 match resolve_name(s) {
                                     Ok(a) => Ok(a),
                                     Err(e) => {
                                         println!("Could not resolve TARGET argument {}: {}", s, e);
                                         Err(e)
                                     }
                                 }
                             })
                             .collect::<Vec<SocketAddr>>();

    if target_addrs.len() != config.target_addrs.len() {
        Err(IOError::new(ErrorKind::NotFound, "Could not resolve target address"))
    } else {
        Ok(Backend::new(target_addrs))
    }
}

fn make_frontend(config: &FrontendConfig,
                 backends: &HashMap<&String, Rc<RefCell<Backend>>>)
                 -> IOResult<Rc<Frontend>> {
    Ok(Frontend::new(try!(resolve_name(&config.listen_addr)),
                     vec![backends[&config.backend].clone()]))
}
