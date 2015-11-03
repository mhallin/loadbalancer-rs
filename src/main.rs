#![cfg_attr(feature="dev", allow(unstable_features))]
#![cfg_attr(feature="dev", feature(plugin))]
#![cfg_attr(featrue="dev", plugin(clippy))]

extern crate clap;
extern crate mio;
extern crate slab;

#[macro_use]
extern crate log;
extern crate env_logger;

mod connection;
mod frontend;
mod backend;
mod driver;

use std::net::{ToSocketAddrs, SocketAddr};
use std::io::Result as IOResult;

use clap::{Arg, App};
use mio::EventLoop;

use frontend::Frontend;
use backend::Backend;
use driver::Driver;

fn resolve_name(s: &str) -> IOResult<SocketAddr> {
    let addrs: Vec<SocketAddr> = try!(s.to_socket_addrs()).collect();

    assert_eq!(addrs.len(), 1);

    Ok(addrs[0])
}

fn main() {
    env_logger::init().unwrap();

    let matches = App::new("loadbalancer")
                      .version(env!("CARGO_PKG_VERSION"))
                      .author("Magnus Hallin <mhallin@fastmail.com>")
                      .about("TCP load balancer")
                      .arg(Arg::with_name("LISTEN")
                               .help("Listen address of the load balancer")
                               .required(true)
                               .index(1))
                      .arg(Arg::with_name("TARGET")
                               .help("Target adresses")
                               .required(true)
                               .index(2)
                               .multiple(true))
                      .get_matches();

    let listen_addr = match resolve_name(matches.value_of("LISTEN")
                                                .expect("Must provide LISTEN argument")) {
        Ok(l) => l,
        Err(e) => {
            println!("Could not resolve LISTEN argument: {}", e);
            return;
        }
    };

    let target_names = matches.values_of("TARGET")
                              .expect("Must provide one or more TARGET arguments");
    let num_targets = target_names.len();

    let target_addrs: Vec<SocketAddr> = target_names.into_iter()
                                                    .flat_map(|s| {
                                                        match resolve_name(s) {
                                                            Ok(a) => Some(a),
                                                            Err(e) => {
                                                                println!("Could not resolve \
                                                                          TARGET argument {}: {}",
                                                                         s,
                                                                         e);
                                                                None
                                                            }
                                                        }
                                                    })
                                                    .collect();

    if num_targets != target_addrs.len() {
        return;
    }

    info!("Using listen address: {:?}", listen_addr);
    info!("Using targets: {:?}", target_addrs);

    let backend = Backend::new(target_addrs);
    let frontend = Frontend::new(listen_addr, vec![backend]);
    let mut driver = Driver::new();
    let mut event_loop = EventLoop::new().unwrap();

    driver.register(&mut event_loop, frontend).unwrap();

    trace!("Starting event loop");
    event_loop.run(&mut driver).unwrap();
}
