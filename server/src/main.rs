mod cli;
mod config;
mod display;
mod server;
mod weather;
mod wl;

use clap::Parser;

use crate::cli::Cli;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::init();
    let cli = Cli::parse();
    cli.run().await;
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs::File, io::BufReader};

    use interface::{Configuration, Element, FontName, Point, Screen, TextStyle};
    use jsonschema;
    use schemars::schema_for;
    use serde_json;

    /// validate the test json file against the schema
    #[test]
    fn test_schema_validation_passing() {
        let schema = schema_for!(Configuration);
        let schema_json = serde_json::to_value(&schema).unwrap();
        let file = File::open("test.json").expect("Failed to read test.json");
        let reader = BufReader::new(file);
        let instance: serde_json::Value = serde_json::from_reader(reader).unwrap();

        assert!(jsonschema::is_valid(&schema_json, &instance));
    }

    #[test]
    fn test_serialize_deserialize_config_with_postcard() {
        let config = Configuration {
            text_styles: BTreeMap::from([(
                "normal".into(),
                TextStyle {
                    text_color: "FFFFFF".into(),
                    font: FontName::Font5X7,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                },
            )]),
            screens: vec![Screen {
                elements: vec![Element::Text {
                    position: Point { x: 50, y: 20 },
                    style: "style".into(),
                    text: "content".into(),
                    align: None,
                }],
            }],
        };
        let buf = postcard::to_allocvec(&config).unwrap();
        let config2: Configuration = postcard::from_bytes(&buf).unwrap();
        assert_eq!(config, config2);
    }
}
