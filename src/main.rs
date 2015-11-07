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

use clap::{Arg, App};
use mio::EventLoop;

use config::RootConfig;
use driver_state::DriverState;
use driver::Driver;

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

    let mut event_loop = EventLoop::new().unwrap();

    let mut driver_state = DriverState::new(&config.buffers);
    driver_state.reconfigure(&mut event_loop, config).unwrap();

    let mut driver = Driver::new(driver_state);

    info!("Starting event loop");

    event_loop.run(&mut driver).unwrap()
}
