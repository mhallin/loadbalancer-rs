use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Error as IOError};
use std::result::Result;
use std::default::Default;

use rustc_serialize::Decodable;
use toml;

#[derive(Debug, RustcDecodable, Default)]
pub struct RootConfig {
    pub frontends: HashMap<String, FrontendConfig>,
    pub backends: HashMap<String, BackendConfig>,
    pub buffers: BufferConfig,
}

#[derive(Debug, RustcDecodable, Default)]
pub struct FrontendConfig {
    pub listen_addr: String,
    pub backend: String,
}

#[derive(Debug, RustcDecodable, Default)]
pub struct BackendConfig {
    pub target_addrs: Vec<String>,
}

#[derive(Debug, RustcDecodable)]
pub struct BufferConfig {
    pub connections: usize,
    pub listeners: usize,
}

#[derive(Debug)]
pub enum ReadError {
    IOError(IOError),
    ParseError(Vec<toml::ParserError>),
    DecodeError(toml::DecodeError),
}

impl RootConfig {
    pub fn from_str(config: &str) -> Result<RootConfig, ReadError> {
        let mut parser = toml::Parser::new(&config);
        let table = try!(parser.parse().ok_or(parser.errors.clone()));
        let mut decoder = toml::Decoder::new(toml::Value::Table(table));

        let config = try!(RootConfig::decode(&mut decoder));

        Ok(config)
    }

    pub fn read_config(filename: &str) -> Result<RootConfig, ReadError> {
        let mut contents = String::new();
        let mut file = try!(File::open(filename));
        try!(file.read_to_string(&mut contents));

        RootConfig::from_str(&contents)
    }
}

impl From<IOError> for ReadError {
    fn from(e: IOError) -> ReadError {
        ReadError::IOError(e)
    }
}

impl From<toml::DecodeError> for ReadError {
    fn from(e: toml::DecodeError) -> ReadError {
        ReadError::DecodeError(e)
    }
}

impl From<Vec<toml::ParserError>> for ReadError {
    fn from(e: Vec<toml::ParserError>) -> ReadError {
        ReadError::ParseError(e)
    }
}

impl Default for BufferConfig {
    fn default() -> Self {
        BufferConfig {
            connections: 4096,
            listeners: 128,
        }
    }
}
