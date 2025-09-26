use std::{collections::HashMap, time::Duration};

use anyhow::{Result, anyhow};
use chrono::{DateTime, Local};
use log::{info, warn};
use serde::Deserialize;

use crate::server::WEATHER_POLL_RATE;

const WEATHER_BASE_URL: &str = "https://api.met.no/weatherapi/locationforecast/2.0/complete";
const USER_AGENT: &str = "https://github.com/florianL21/headless-led-wall";

#[derive(Debug, Clone)]
pub enum WeatherUpdateResult {
    Updated(WeatherData, Duration),
    Unchanged(Duration),
}

#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct WeatherData {
    pub six_hour_forecast: WeatherForecast,
    pub twelve_hour_forecast: WeatherForecast,
    pub hourly_forecast: Vec<WeatherForecast>,
}

#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct WeatherForecast {
    pub time: DateTime<Local>,
    pub chance_of_rain: f32,
    pub precipitation_amount: f32,
    pub air_temperature: f32,
    pub symbol: String,
    pub max_temp: f32,
    pub min_temp: f32,
}

impl WeatherForecast {
    fn from_forecast(forecast: &Forecast, time: DateTime<Local>) -> Self {
        WeatherForecast {
            time,
            chance_of_rain: forecast
                .details
                .probability_of_precipitation
                .unwrap_or_default(),
            precipitation_amount: forecast.details.precipitation_amount.unwrap_or_default(),
            air_temperature: forecast.details.air_temperature_max.unwrap_or_default(),
            max_temp: forecast.details.air_temperature_max.unwrap_or_default(),
            min_temp: forecast.details.air_temperature_min.unwrap_or_default(),
            symbol: forecast.summary.symbol_code.clone(),
        }
    }

    fn from_timentry(entry: &TimeEntry) -> Option<Self> {
        let one_h = entry.data.next_1_hours.as_ref()?;
        let air_temp = entry
            .data
            .instant
            .details
            .air_temperature
            .unwrap_or_default();
        Some(WeatherForecast {
            time: DateTime::parse_from_rfc3339(&entry.time)
                .unwrap_or_default()
                .into(),
            chance_of_rain: one_h
                .details
                .probability_of_precipitation
                .unwrap_or_default(),
            precipitation_amount: one_h.details.precipitation_amount.unwrap_or_default(),
            air_temperature: air_temp,
            min_temp: air_temp,
            max_temp: air_temp,
            symbol: one_h.summary.symbol_code.clone(),
        })
    }
}

#[derive(Deserialize, Debug)]
pub struct Response {
    properties: Properties,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct Properties {
    meta: Metadata,
    timeseries: Vec<TimeEntry>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct Metadata {
    updated_at: String,
    units: MetaUnits,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct MetaUnits {
    air_pressure_at_sea_level: String,
    air_temperature: String,
    cloud_area_fraction: String,
    precipitation_amount: String,
    relative_humidity: String,
    wind_from_direction: String,
    wind_speed: String,
}

#[derive(Deserialize, Debug)]
pub struct TimeEntry {
    time: String,
    data: TimeseriesData,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct TimeseriesData {
    instant: WeatherInstant,
    next_12_hours: Option<Forecast>,
    next_6_hours: Option<Forecast>,
    next_1_hours: Option<Forecast>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct WeatherInstant {
    details: InstantDetails,
}

/// These are valid for a specific point in time, and can be found under instant.
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct InstantDetails {
    /// air pressure at sea level
    /// ### in hPa
    air_pressure_at_sea_level: Option<f32>,
    /// air temperature at 2m above the ground
    /// ### in °C
    air_temperature: Option<f32>,
    /// 10th percentile of air temperature (i.e 90% chance it will be above this value)
    /// ### in °C
    air_temperature_percentile_10: Option<f32>,
    /// 90th percentile of air temperature (i.e 10% chance it will be above this value)
    /// ### in °C
    air_temperature_percentile_90: Option<f32>,
    /// total cloud cover for all heights
    /// ### in %
    cloud_area_fraction: Option<f32>,
    /// cloud cover higher than 5000m above the ground
    /// ### in %
    cloud_area_fraction_high: Option<f32>,
    /// cloud cover lower than 2000m above the ground
    /// ### in %
    cloud_area_fraction_low: Option<f32>,
    /// cloud cover between 2000 and 5000m above the ground
    /// ### in %
    cloud_area_fraction_medium: Option<f32>,
    /// dew point temperature 2m above the ground
    /// ### in °C
    dew_point_temperature: Option<f32>,
    /// amount of surrounding area covered in fog (horizontal view under a 1000 meters)
    /// ### in %
    fog_area_fraction: Option<f32>,
    /// relative humidity at 2m above the ground
    /// ### in %
    relative_humidity: Option<f32>,
    /// ultraviolet index for cloud free conditions, 0 (low) to 11+ (extreme)
    ultraviolet_index_clear_sky: Option<f32>,
    /// direction the wind is coming from (0° is north, 90° east, etc.)
    /// ### in degrees
    wind_from_direction: Option<f32>,
    /// wind speed at 10m above the ground (10 min average)
    /// ### in m/s
    wind_speed: Option<f32>,
    /// 10th percentile of wind speed at 10m above the ground (10 min average)
    /// ### in m/s
    wind_speed_percentile_10: Option<f32>,
    /// 90th percentile of wind speed at 10m above the ground (10 min average)
    /// ### in m/s
    wind_speed_percentile_90: Option<f32>,
    /// maximum gust for period at 10m above the ground. Gust is wind speed averaged over 3s
    /// ### in m/s
    wind_speed_of_gust: Option<f32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct Forecast {
    summary: ForecastSummary,
    details: ForecastDetails,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone, Default)]
pub struct ForecastSummary {
    symbol_code: String,
    symbol_confidence: Option<String>,
}

/// These are aggregations or minima/maxima for a given time period,
/// either the next 1, 6 or 12 hours. Not that next_1_hours is only
/// available in the short range forecast.
#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone, Default)]
pub struct ForecastDetails {
    /// ### in °C
    /// maximum air temperature over period
    air_temperature_max: Option<f32>,
    /// ### in °C
    /// minimum air temperature over period
    air_temperature_min: Option<f32>,
    /// ### in mm
    /// expected precipitation amount for period
    precipitation_amount: Option<f32>,
    /// ### in mm
    /// maximum likely precipitation for period
    precipitation_amount_max: Option<f32>,
    /// ### in mm
    /// minimum likely precipitation for period
    precipitation_amount_min: Option<f32>,
    /// ### in %
    /// chance of precipitation during period
    probability_of_precipitation: Option<f32>,
    /// ### in %
    /// chance of thunder during period
    probability_of_thunder: Option<f32>,
}

fn add_params(
    req: reqwest::RequestBuilder,
    params: &HashMap<String, String>,
) -> reqwest::RequestBuilder {
    req.query(params).header("User-Agent", USER_AGENT)
}

fn calc_next_update(resp: &reqwest::Response) -> Duration {
    let mut next_check = WEATHER_POLL_RATE;
    if let Some(expires) = resp.headers().get("Expires")
        && let Ok(expires) = expires.to_str()
        && let Ok(expires) = DateTime::parse_from_rfc2822(expires)
    {
        let delta = expires.timestamp() - Local::now().timestamp();
        if delta > 0 {
            next_check = Duration::from_secs(delta as u64);
        }
    }
    next_check
}

pub async fn get_weather_data(
    last_updated: &mut Option<DateTime<Local>>,
    client: &reqwest::Client,
    api_params: &HashMap<String, String>,
) -> Result<WeatherUpdateResult> {
    if let Some(last_updated) = last_updated {
        let resp = add_params(
            client
                .head(WEATHER_BASE_URL)
                .header("If-Modified-Since", last_updated.to_rfc2822()),
            api_params,
        )
        .send()
        .await?;

        if resp.status() == 304 {
            let next_check = calc_next_update(&resp);
            // indicate that there was no update
            return Ok(WeatherUpdateResult::Unchanged(next_check));
        }
    }
    // Update is needed from the API, so go and fetch it
    let resp = add_params(client.get(WEATHER_BASE_URL), api_params)
        .send()
        .await?;

    let status = resp.status();
    if status == 203 {
        warn!("Using a deprecated or beta model");
    } else if status != 200 {
        let text = resp.text().await?;
        return Err(anyhow!("Failed to fetch data from Weather API: {text}"));
    }

    let next_check = calc_next_update(&resp);

    let weather_data: Response = resp.json().await?;
    let data = condense(weather_data);
    *last_updated = Some(Local::now());
    info!("Updated weather data");
    Ok(WeatherUpdateResult::Updated(data, next_check))
}

fn condense(mut data: Response) -> WeatherData {
    let mut result = WeatherData::default();
    // make sure the forecast is sorted by time
    data.properties
        .timeseries
        .sort_by(|a, b| a.time.cmp(&b.time));
    let six_hour_forecast_entry = data
        .properties
        .timeseries
        .iter()
        .find(|e| e.data.next_6_hours.is_some());
    let hourly_forecast: Vec<_> = data.properties.timeseries.iter().take(8).collect();
    if let Some(entry) = six_hour_forecast_entry
        && let Some(ref six_hour_forecast) = entry.data.next_6_hours
    {
        result.six_hour_forecast = WeatherForecast::from_forecast(
            six_hour_forecast,
            DateTime::parse_from_rfc3339(&entry.time)
                .unwrap_or_default()
                .into(),
        );
    }

    let twelve_hour_forecast_entry = data
        .properties
        .timeseries
        .iter()
        .find(|e| e.data.next_12_hours.is_some());
    if let Some(entry) = twelve_hour_forecast_entry
        && let Some(ref twelve_hour_forecast) = entry.data.next_12_hours
    {
        result.twelve_hour_forecast = WeatherForecast::from_forecast(
            twelve_hour_forecast,
            DateTime::parse_from_rfc3339(&entry.time)
                .unwrap_or_default()
                .into(),
        );
    }

    for entry in hourly_forecast {
        let forecast = WeatherForecast::from_timentry(entry);
        if let Some(forecast) = forecast {
            result.hourly_forecast.push(forecast);
        }
    }
    result
}
