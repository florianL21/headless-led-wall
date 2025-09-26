use chrono::prelude::*;

use interface::{Alignment, Configuration, Element, FontName, Point, Screen, TextStyle};

use crate::{
    weather::{WeatherData, WeatherForecast},
    wl::TransportData,
};

fn map(x: f32, in_min: f32, in_max: f32, out_min: i32, out_max: i32) -> i32 {
    ((x - in_min) * (out_max - out_min) as f32 / (in_max - in_min)) as i32 + out_min
}

pub fn build_display(weather_data: &WeatherData, transport_data: &TransportData) -> Configuration {
    let now = Local::now();
    // Render Wiener linien data
    let clock = now.format("%H:%M").to_string();
    let mut elements = vec![
        Element::new_text("clock", clock, Point::new(2, 13)),
        // Separator between top and bottom section of the display
        Element::new_line(Point::new(0, 19), Point::new(192, 19), "FFFFFF").with_stroke(3),
    ];
    let mut y_offset = 32;
    let y_size = 12;
    // limited to 6 as only 6 fit onto the display
    for line in transport_data.lines.iter().take(6) {
        let t = format!("{}{}", line.line.clone(), line.direction_letter);
        elements.push(Element::new_sprite(t, Point::new(2, y_offset - 9)));
        // Direction
        let mut dir = line.direction.clone();
        dir.truncate(17);
        elements.push(Element::new_text("arrival", dir, Point::new(28, y_offset)));
        let time = line.times.clone().into_iter().filter(|v| v > &1);
        let times: Vec<String> = time
            .take(2)
            .map(|v| format!("{:>2}", v.to_string()))
            .collect();
        let times = times.join("/");
        elements.push(
            Element::new_text("arrival", times, Point::new(190, y_offset))
                .with_alignment(Alignment::Right),
        );
        y_offset += y_size;
    }

    // render weather data
    const X_START: i32 = 56;
    const X_END: i32 = 136;
    const NUM_POINTS: i32 = 8;
    const Y_MIN: i32 = 16;
    const Y_MAX: i32 = 1;
    const X_STEP: i32 = (X_END - X_START) / NUM_POINTS;
    let mut curr_x = X_START;

    let forecast_iter = weather_data
        .hourly_forecast
        .iter()
        .take(NUM_POINTS as usize);
    let comparator = |a: &&WeatherForecast, b: &&WeatherForecast| {
        a.air_temperature.partial_cmp(&b.air_temperature).unwrap()
    };
    let min = forecast_iter
        .clone()
        .min_by(comparator)
        .map(|v| v.air_temperature)
        .unwrap_or_default();
    let max = forecast_iter
        .clone()
        .max_by(comparator)
        .map(|v| v.air_temperature)
        .unwrap_or_default();

    elements.push(Element::new_sprite(
        weather_data.six_hour_forecast.symbol.clone(),
        Point::new(175, 1),
    ));

    let mut graph_points: Vec<Point> = Vec::new();
    for forecast in forecast_iter {
        let y = map(forecast.air_temperature, min, max, Y_MIN, Y_MAX);
        graph_points.push(Point::new(curr_x, y));
        elements.push(Element::new_line(
            Point::new(curr_x, 17),
            Point::new(curr_x, 16),
            "404040",
        ));
        curr_x += X_STEP;
    }
    curr_x -= X_STEP;

    elements.push(Element::new_text(
        "weather_hl",
        format!("{max:2.1}°"),
        Point::new(curr_x + 2, 7),
    ));
    elements.push(Element::new_text(
        "weather_hl",
        format!("{min:2.1}°"),
        Point::new(curr_x + 2, 15),
    ));

    elements.push(Element::new_polyline(graph_points, "FFFFFF"));
    // Separator between clock and temp history
    elements.push(
        Element::new_line(Point::new(X_START, 0), Point::new(X_START, 17), "FFFFFF").with_stroke(1),
    );

    Configuration::new(vec![Screen { elements }])
        .add_style("clock", TextStyle::new("FFFFFF", FontName::Font7X13Bold))
        .add_style("arrival", TextStyle::new("FFFFFF", FontName::Font7X13Bold))
        .add_style("weather_hl", TextStyle::new("FFFFFF", FontName::Font5X7))
}
