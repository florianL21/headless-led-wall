use std::collections::HashMap;

use icu_casemap::{TitlecaseMapper, options::TitlecaseOptions};
use icu_locale_core::langid;

use log::{debug, info};
use reqwest::Error;
use serde::Deserialize;

const WL_MONITOR_BASE: &str = "https://www.wienerlinien.at/ogd_realtime/monitor";
const INTERRUPTIONS_PARAMS: &str =
    "activateTrafficInfo=stoerunglang&activateTrafficInfo=stoerungkurz";

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TransportData {
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub line: String,
    pub direction: String,
    pub direction_letter: String,
    pub times: Vec<u32>,
}

#[derive(Deserialize, Debug)]
pub struct Response {
    data: Monitor,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct Monitor {
    monitors: Vec<MonitorData>,
    #[serde(rename = "trafficInfos")]
    traffic_infos: Vec<TrafficInfo>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct TrafficInfo {
    name: String,
    title: String,
    #[serde(rename = "relatedLines")]
    related_lines: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct MonitorData {
    lines: Vec<LineData>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct LineData {
    name: String,
    towards: String,
    #[serde(rename = "richtungsId")]
    richtungs_id: String,
    /// "H" or "R"
    direction: String,
    #[serde(rename = "realtimeSupported")]
    realtime_supported: bool,
    departures: DepartureWrapper,
}

#[derive(Deserialize, Debug)]
pub struct DepartureWrapper {
    departure: Vec<DepartureData>,
}

#[derive(Deserialize, Debug)]
pub struct DepartureData {
    #[serde(rename = "departureTime")]
    departure_time: DepartureTime,
}

#[derive(Deserialize, Debug)]
pub struct DepartureTime {
    countdown: u32,
}

fn capitalize(text: String) -> String {
    let cm = TitlecaseMapper::new();
    let root = langid!("de-at");
    let mut default_options: TitlecaseOptions = Default::default();
    default_options.leading_adjustment = Some(icu_casemap::options::LeadingAdjustment::ToCased);
    let lower = text.to_lowercase();
    let parts: Vec<String> = lower
        .split(" ")
        .map(|s| {
            cm.titlecase_segment_to_string(s, &root, default_options)
                .to_string()
        })
        .collect();
    parts.join(" ")
}

pub async fn get_transport_data(
    client: &reqwest::Client,
    station_query: &String,
    line_filter: &HashMap<String, u32>,
) -> Result<TransportData, Error> {
    let res = client
        .post(format!(
            "{WL_MONITOR_BASE}?{station_query}&{INTERRUPTIONS_PARAMS}"
        ))
        .send()
        .await?;
    let data: Response = res.json().await?;
    debug!("Response: {data:#?}");

    let mut infos = Vec::new();
    for monitor in data.data.monitors {
        for line in monitor.lines {
            let line_name = line.name.as_str();
            if line_filter.contains_key(line_name) {
                let mut current = Line {
                    line: line.name.clone(),
                    direction: capitalize(line.towards),
                    direction_letter: line.direction.to_uppercase(),
                    times: Vec::new(),
                };
                let min_time = line_filter.get(line_name);
                for departure in line.departures.departure {
                    if departure.departure_time.countdown >= *min_time.unwrap_or(&0) {
                        current.times.push(departure.departure_time.countdown);
                    }
                }
                if !current.times.is_empty() {
                    infos.push(current);
                }
            }
        }
    }

    infos.sort_by_key(|e| *e.times.first().unwrap_or(&u32::MAX));

    let parsed_data = TransportData { lines: infos };
    info!("Updated Winer linen data");

    Ok(parsed_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that text which comes over the API in various cases is capitalized correctly
    #[test]
    fn test_text_capitalization_1() {
        assert_eq!(capitalize("test text".to_string()), "Test Text");
    }

    #[test]
    fn test_text_capitalization_2() {
        assert_eq!(capitalize("TEST TEXT".to_string()), "Test Text");
    }

    #[test]
    fn test_text_capitalization_3() {
        assert_eq!(capitalize("Test text".to_string()), "Test Text");
    }

    #[test]
    fn test_text_capitalization() {
        assert_eq!(capitalize("TEST".to_string()), "Test");
    }
}
