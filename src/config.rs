use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Error as IOError};
use std::result::Result;

use rustc_serialize::Decodable;
use toml;

#[derive(Debug, RustcDecodable)]
pub struct RootConfig {
    pub frontends: HashMap<String, FrontendConfig>,
    pub backends: HashMap<String, BackendConfig>,
}

#[derive(Debug, RustcDecodable)]
pub struct FrontendConfig {
    pub listen_addr: String,
    pub backend: String,
}

#[derive(Debug, RustcDecodable)]
pub struct BackendConfig {
    pub target_addrs: Vec<String>,
}

#[derive(Debug)]
pub enum ReadError {
    IOError(IOError),
    ParseError(Vec<toml::ParserError>),
    DecodeError(toml::DecodeError),
}

impl RootConfig {
    pub fn read_config(filename: &str) -> Result<RootConfig, ReadError> {
        let mut contents = String::new();
        let mut file = try!(File::open(filename));
        try!(file.read_to_string(&mut contents));

        let mut parser = toml::Parser::new(&contents);
        let table = try!(parser.parse().ok_or(parser.errors.clone()));
        let mut decoder = toml::Decoder::new(toml::Value::Table(table));

        let config = try!(RootConfig::decode(&mut decoder));

        Ok(config)
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
