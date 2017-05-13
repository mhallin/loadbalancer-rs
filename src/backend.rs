use std::net::SocketAddr;
use std::rc::Rc;
use std::cell::RefCell;

pub struct Backend {
    targets: Vec<SocketAddr>,
    next_target: usize,
}

impl Backend {
    pub fn new(targets: Vec<SocketAddr>) -> Rc<RefCell<Backend>> {
        Rc::new(RefCell::new(Backend {
                                 targets: targets,
                                 next_target: 0,
                             }))
    }

    pub fn decide_target(&mut self) -> SocketAddr {
        let target = self.targets[self.next_target];
        self.next_target = (self.next_target + 1) % self.targets.len();

        target
    }
}
