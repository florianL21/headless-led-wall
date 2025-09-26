use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct DisplayConfig {
    pub ip: Ipv4Addr,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerConfig {
    /// WinerLinien API query parameters
    pub wl: HashMap<String, Vec<String>>,
    /// MET institute API query parameters
    pub met: HashMap<String, String>,
    /// Line filter for the transport info from the WL API
    pub line_filter: HashMap<String, u32>,
    /// Settings for the display
    pub display: DisplayConfig,
}

impl ServerConfig {
    pub fn from_toml(file: PathBuf) -> Self {
        let mut f = File::open(&file).expect("Could not open server config file");
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .expect("Could not read data from server config file");

        toml::from_slice(&buf).expect("Could not parse ServerConfig toml file")
    }

    pub fn build_wl_query(&self) -> String {
        let mut params = Vec::new();
        for (key, values) in self.wl.iter() {
            for value in values {
                params.push(format!("{key}={value}"));
            }
        }
        params.join("&")
    }
}
