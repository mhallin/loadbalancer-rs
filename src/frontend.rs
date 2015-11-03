use std::net::SocketAddr;
use std::rc::Rc;
use std::cell::RefCell;

use backend::Backend;

pub struct Frontend {
    listen_addr: SocketAddr,
    backends: Vec<Rc<RefCell<Backend>>>,
}

impl Frontend {
    pub fn new(listen_addr: SocketAddr, backends: Vec<Rc<RefCell<Backend>>>) -> Rc<Frontend> {
        Rc::new(Frontend {
            listen_addr: listen_addr,
            backends: backends,
        })
    }

    pub fn listen_addrs(&self) -> Vec<SocketAddr> {
        vec![self.listen_addr]
    }

    pub fn decide_backend(&self) -> Rc<RefCell<Backend>> {
        self.backends[0].clone()
    }
}
