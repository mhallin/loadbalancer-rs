#![cfg_attr(feature="dev", allow(unstable_features))]
#![cfg_attr(feature="dev", feature(plugin))]
#![cfg_attr(featrue="dev", plugin(clippy))]

extern crate clap;
extern crate mio;
extern crate slab;
extern crate toml;
extern crate rustc_serialize;

#[macro_use]
extern crate log;
extern crate env_logger;

mod config;
mod connection;
mod frontend;
mod backend;
mod driver_state;
mod driver;

use std::net::{ToSocketAddrs, SocketAddr};
use std::io::{ErrorKind, Result as IOResult, Error as IOError};
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;

use clap::{Arg, App};
use mio::EventLoop;

use config::{RootConfig, FrontendConfig, BackendConfig};
use frontend::Frontend;
use backend::Backend;
use driver_state::DriverState;
use driver::Driver;

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

fn main() {
    env_logger::init().unwrap();

    let matches = App::new("loadbalancer")
                      .version(env!("CARGO_PKG_VERSION"))
                      .author("Magnus Hallin <mhallin@fastmail.com>")
                      .about("TCP load balancer")
                      .arg(Arg::with_name("CONFIG")
                               .short("c")
                               .long("config")
                               .help("Listen address of the load balancer")
                               .required(true)
                               .takes_value(true))
                      .get_matches();

    let config_path = matches.value_of("CONFIG").expect("Config parameter must be set");

    let config = RootConfig::read_config(&config_path).unwrap();

    debug!("Using config: {:#?}", config);

    let mut backends = HashMap::new();
    let mut frontends = HashMap::new();

    for (name, config) in config.backends.iter() {
        backends.insert(name, make_backend(config).unwrap());
    }

    for (name, config) in config.frontends.iter() {
        frontends.insert(name, make_frontend(config, &backends).unwrap());
    }

    let mut driver = Driver::new(DriverState::new());
    let mut event_loop = EventLoop::new().unwrap();

    for (_, frontend) in frontends.into_iter() {
        driver.register(&mut event_loop, frontend).unwrap();
    }

    info!("Starting event loop");

    event_loop.run(&mut driver).unwrap()
}
