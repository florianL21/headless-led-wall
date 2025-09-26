use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use indicatif::ProgressIterator;
use interface::{Configuration, Resource};
use log::{error, info, warn};
use postcard::to_allocvec;
use schemars::schema_for;
use serde::Deserialize;
use static_cell::StaticCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::{fs, net::Ipv4Addr, path::PathBuf};
use tokio::signal;
use tokio::signal::unix::SignalKind;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::config::ServerConfig;
use crate::server::{
    DataUpdate, fetch_transport_data, fetch_weather_data, maintain_display, push_display_update,
};

/// Run a display server for public transport information
/// This server will push updated display configurations to the specified client
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path to config file for the display to update
    config: PathBuf,
    /// Action to execute
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate the jsonschema reference for the display configuration
    GenerateSchema { output_file: PathBuf },

    /// Run the server which will fetch information from various external sources
    /// and update the display on the given IP with it.
    Server,

    /// Try to parse a given json file and provide details issues with the json content
    TryParse { input_file: PathBuf },

    /// Convert the given json file to the postcard wire format
    ToPostcard {
        input_file: PathBuf,
        output_file: PathBuf,
    },

    /// Push a configuration in json format to the display server
    PushConfig {
        /// Configuration json file to push
        input_file: PathBuf,
    },

    /// Upload a sprite to the display server
    UploadSprite {
        /// Name of the sprite
        name: String,
        /// Time that each frame should be displayed in ms
        frame_time: u16,
        /// QOI image file which make up all frames of the sprite
        input_files: Vec<PathBuf>,
    },

    /// Bulk upload all sprites from a sprites.toml file
    BulkUpload {
        /// Path to the sprites.toml file which contains all meta information about all the sprites
        meta_file: PathBuf,
        /// If specified issue a format command before uploading all the sprites
        #[arg(long)]
        format: bool,
        /// Filter the list of sprites to upload, can be specified multiple times
        #[arg(short, long)]
        filter: Option<Vec<String>>,
    },
}

type SpriteCollection = HashMap<String, SpriteDefinition>;

#[derive(Debug, Deserialize)]
struct SpriteDefinition {
    frames: Vec<PathBuf>,
    frame_time: u16,
}

impl Cli {
    pub async fn run(self) {
        let conf = ServerConfig::from_toml(self.config);
        let ip = conf.display.ip;
        match self.command {
            Commands::GenerateSchema { output_file } => {
                let schema = schema_for!(Configuration);
                fs::write(
                    output_file,
                    serde_json::to_string_pretty(&schema).expect("Failed to serialize schema"),
                )
                .expect("Failed to write schema to file");
            }
            Commands::Server => {
                info!("Running server pushing updates to IP: {}", ip);

                let (tx, rx) = mpsc::channel::<DataUpdate>(100);
                let mut set = JoinSet::new();
                let token = CancellationToken::new();

                static CLIENT: StaticCell<reqwest::Client> = StaticCell::new();
                let client: &'static mut reqwest::Client = CLIENT.init(reqwest::Client::new());

                set.spawn(fetch_transport_data(
                    token.clone(),
                    tx.clone(),
                    client,
                    conf.clone(),
                ));
                set.spawn(fetch_weather_data(
                    token.clone(),
                    tx.clone(),
                    client,
                    conf.clone(),
                ));
                set.spawn(push_display_update(token.clone(), ip, rx));
                set.spawn(maintain_display(token.clone(), tx));

                #[cfg(not(target_family = "unix"))]
                signal::ctrl_c()
                    .await
                    .expect("Failed to setup ctrl+c listener");
                #[cfg(target_family = "unix")]
                {
                    let mut signal = signal::unix::signal(SignalKind::terminate())
                        .expect("Failed to setup sigterm listener");
                    signal.recv().await;
                }
                info!("Shutting down...");
                token.cancel();
                set.join_all().await;
                info!("All tasks exited");
            }
            Commands::TryParse { input_file } => {
                let f = File::open(input_file).expect("Could not open file");
                let reader = BufReader::new(f);
                let parsed: Configuration =
                    serde_json::from_reader(reader).expect("Could not parse json");
                info!("{parsed:#?}");
            }
            Commands::ToPostcard {
                input_file,
                output_file,
            } => {
                let f = File::open(input_file).expect("Could not open file");
                let reader = BufReader::new(f);
                let parsed: Configuration =
                    serde_json::from_reader(reader).expect("Could not parse json");
                let output: Vec<u8> =
                    to_allocvec(&parsed).expect("Could not convert to postcard format");
                fs::write(output_file, output).expect("Could not write output file");
            }
            Commands::PushConfig { input_file } => {
                let f = File::open(input_file).expect("Could not open file");
                let reader = BufReader::new(f);
                let parsed: Configuration =
                    serde_json::from_reader(reader).expect("Could not parse json");
                let buf = postcard::to_allocvec(&parsed)
                    .expect("Could not serialize configuration to postcard format");
                let client = reqwest::Client::new();
                let res = client
                    .post(format!("http://{ip}/api/config"))
                    .body(buf)
                    .send()
                    .await
                    .expect("Failed to send request");
                let status = res.status();
                if status.is_success() {
                    info!("Success {}: {:#?}", status, res.text().await);
                } else {
                    error!("Error: {:#?}", res.text().await);
                }
            }
            Commands::UploadSprite {
                name,
                input_files,
                frame_time,
            } => {
                let client = reqwest::Client::new();
                sprite_upload(&client, &input_files, &ip, &name, frame_time).await;
            }
            Commands::BulkUpload {
                meta_file,
                format,
                filter,
            } => {
                let mut config = get_sprites(&meta_file);
                if let Some(filter) = filter {
                    config = config
                        .into_iter()
                        .filter(|s| filter.contains(&s.0))
                        .collect();
                }
                let client = reqwest::Client::new();
                if format {
                    info!("Formatting flash. This may take a while...");
                    format_flash(&client, &ip)
                        .await
                        .expect("Failed to format flash");
                }
                info!("Uploading {} sprites", config.len());
                let base_path = meta_file
                    .parent()
                    .expect("Could not get folder of metadata file");
                for (name, sprite) in config.iter().progress() {
                    let files: Vec<_> = sprite.frames.iter().map(|x| base_path.join(x)).collect();
                    sprite_upload(&client, &files, &ip, name, sprite.frame_time).await;
                }
            }
        }
    }
}

async fn sprite_upload(
    client: &reqwest::Client,
    input_files: &Vec<PathBuf>,
    ip: &Ipv4Addr,
    name: &String,
    frame_time: u16,
) {
    let mut frames: Vec<Vec<u8>> = Vec::new();
    for input_file in input_files {
        let mut f = File::open(input_file).expect("Could not open file");
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .expect("Could not read data from file");
        frames.push(buf);
    }
    let sprite = Resource::new(frames, frame_time);
    let buf =
        postcard::to_allocvec(&sprite).expect("Could not serialize sprite to postcard format");
    let mut res = None;
    for _ in 0..3 {
        res = Some(
            client
                .post(format!("http://{ip}/api/storage/upload?key={name}"))
                .body(buf.clone())
                .timeout(Duration::from_secs(10))
                .send()
                .await,
        );
        if let Some(Ok(_)) = res {
            break;
        } else {
            warn!("Failed to send sprite data");
        }
    }
    match res {
        Some(Err(e)) => {
            error!("Failed to send sprite data: {e}");
        }
        Some(Ok(res)) => {
            let status = res.status();
            if !status.is_success() {
                error!("Error: {:#?}", res.text().await);
            }
        }
        _ => {}
    }
}

async fn format_flash(client: &reqwest::Client, ip: &Ipv4Addr) -> Result<()> {
    let res = client
        .post(format!("http://{ip}/api/storage/format"))
        .send()
        .await
        .expect("Failed to send request");
    let status = res.status();
    if status.is_success() {
        info!("Success {}: {:#?}", status, res.text().await);
        Ok(())
    } else {
        error!("Error: {:#?}", res.text().await);
        Err(anyhow!("Failed to format flash"))
    }
}

fn get_sprites(meta_file: &Path) -> SpriteCollection {
    let meta_file = meta_file
        .canonicalize()
        .expect("Could not resolve file path");
    let mut f = File::open(&meta_file).expect("Could not open file");
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .expect("Could not read data from file");

    toml::from_slice(&buf).expect("Could not parse toml file")
}
