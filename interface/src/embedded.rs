use super::TextStyle;
use crate::{Alignment, Configuration, Element, FontName, GlobalStylesType, Point, Screen, Size};
use alloc::collections::btree_map::BTreeMap;
use alloc::string::String;
use embedded_graphics::mono_font::iso_8859_1::{
    FONT_4X6, FONT_5X7, FONT_5X8, FONT_6X9, FONT_6X10, FONT_6X12, FONT_6X13, FONT_6X13_BOLD,
    FONT_6X13_ITALIC, FONT_7X13, FONT_7X13_BOLD, FONT_7X13_ITALIC, FONT_7X14, FONT_7X14_BOLD,
    FONT_8X13, FONT_8X13_BOLD, FONT_8X13_ITALIC, FONT_9X15, FONT_9X15_BOLD, FONT_9X18,
    FONT_9X18_BOLD, FONT_10X20,
};
use embedded_graphics::{
    mono_font::{MonoFont, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::Rgb888,
};
use picoserve::response::ErrorWithStatusCode;
use profont::{
    PROFONT_7_POINT, PROFONT_9_POINT, PROFONT_10_POINT, PROFONT_12_POINT, PROFONT_14_POINT,
    PROFONT_18_POINT, PROFONT_24_POINT,
};
use thiserror::Error;

pub type BuiltTextStyles = BTreeMap<String, MonoTextStyle<'static, Rgb888>>;

pub struct CheckedScreenConfig {
    pub screen: Screen,
    pub styles: BuiltTextStyles,
}

impl CheckedScreenConfig {
    pub fn new(config: Configuration) -> Result<Self, ScreenBuildError> {
        if config.screens.len() > 1 {
            Err(ScreenBuildError::TooManyScreens)
        } else if config.screens.is_empty() {
            Err(ScreenBuildError::NoScreen)
        } else if let Some(screen) = config.screens.into_iter().next() {
            let styles = build_styles(config.text_styles)?;
            // TODO: Implement sanity checks to confirm all styles are defined and all sprites are in flash
            Ok(Self { screen, styles })
        } else {
            Err(ScreenBuildError::CouldNotGetScreen)
        }
    }
}

#[derive(Error, Debug, ErrorWithStatusCode)]
pub enum ScreenBuildError {
    #[error("The color string `{0}` was invalid")]
    #[status_code(BAD_REQUEST)]
    InvalidColorString(String),

    #[error("Could not get screen from config")]
    #[status_code(BAD_REQUEST)]
    CouldNotGetScreen,

    #[error("Config must contain at least one screen")]
    #[status_code(BAD_REQUEST)]
    NoScreen,

    #[error("Only configs using a single screen are supported for now")]
    #[status_code(BAD_REQUEST)]
    TooManyScreens,

    #[error("Configuration uses style `{0}` but this style is not defined")]
    #[status_code(BAD_REQUEST)]
    MissingStyle(String),

    #[error("Configuration uses sprite `{0}` but this sprite is not present in flash")]
    #[status_code(BAD_REQUEST)]
    MissingSprite(String),
}

pub fn string_to_color(color: &String) -> Option<Rgb888> {
    Some(Rgb888::new(
        u8::from_str_radix(color.get(0..2)?, 16).ok()?,
        u8::from_str_radix(color.get(2..4)?, 16).ok()?,
        u8::from_str_radix(color.get(4..6)?, 16).ok()?,
    ))
}

impl FontName {
    fn build(self) -> &'static MonoFont<'static> {
        match self {
            FontName::Font4X6 => &FONT_4X6,
            FontName::Font5X7 => &FONT_5X7,
            FontName::Font5X8 => &FONT_5X8,
            FontName::Font6X9 => &FONT_6X9,
            FontName::Font6X10 => &FONT_6X10,
            FontName::Font6X12 => &FONT_6X12,
            FontName::Font6X13 => &FONT_6X13,
            FontName::Font6X13Bold => &FONT_6X13_BOLD,
            FontName::Font6X13Italic => &FONT_6X13_ITALIC,
            FontName::Font7X13 => &FONT_7X13,
            FontName::Font7X13Bold => &FONT_7X13_BOLD,
            FontName::Font7X13Italic => &FONT_7X13_ITALIC,
            FontName::Font7X14 => &FONT_7X14,
            FontName::Font7X14Bold => &FONT_7X14_BOLD,
            FontName::Font8X13 => &FONT_8X13,
            FontName::Font8X13Bold => &FONT_8X13_BOLD,
            FontName::Font8X13Italic => &FONT_8X13_ITALIC,
            FontName::Font9X15 => &FONT_9X15,
            FontName::Font9X15Bold => &FONT_9X15_BOLD,
            FontName::Font9X18 => &FONT_9X18,
            FontName::Font9X18Bold => &FONT_9X18_BOLD,
            FontName::Font10X20 => &FONT_10X20,
            FontName::Profont7 => &PROFONT_7_POINT,
            FontName::Profont9 => &PROFONT_9_POINT,
            FontName::Profont10 => &PROFONT_10_POINT,
            FontName::Profont12 => &PROFONT_12_POINT,
            FontName::Profont14 => &PROFONT_14_POINT,
            FontName::Profont18 => &PROFONT_18_POINT,
            FontName::Profont24 => &PROFONT_24_POINT,
        }
    }
}

impl TextStyle {
    pub fn build(self) -> Result<MonoTextStyle<'static, Rgb888>, ScreenBuildError> {
        let style: MonoTextStyleBuilder<'static, Rgb888> = MonoTextStyleBuilder::new()
            .text_color(string_to_color(&self.text_color).ok_or(
                ScreenBuildError::InvalidColorString(self.text_color.clone()),
            )?)
            .font(self.font.build());
        if let Some(color) = &self.background_color {
            style.background_color(string_to_color(color).ok_or(
                ScreenBuildError::InvalidColorString(self.text_color.clone()),
            )?);
        }
        if let Some(true) = self.strikethrough {
            style.strikethrough();
        }
        if let Some(true) = self.underline {
            style.underline();
        }
        Ok(style.build())
    }
}

pub fn build_styles(styles: GlobalStylesType) -> Result<BuiltTextStyles, ScreenBuildError> {
    styles
        .into_iter()
        .map(|(k, style)| Ok((k, style.build()?)))
        .collect()
}

impl Point {
    pub fn point(&self) -> embedded_graphics::prelude::Point {
        embedded_graphics::prelude::Point::new(self.x, self.y)
    }
}

impl Size {
    pub fn size(&self) -> embedded_graphics::prelude::Size {
        embedded_graphics::prelude::Size::new(self.width, self.height)
    }
}

impl Default for &Point {
    fn default() -> Self {
        &Point { x: 0, y: 0 }
    }
}

impl From<Point> for embedded_graphics::prelude::Point {
    fn from(value: Point) -> Self {
        value.point()
    }
}

impl From<&Point> for embedded_graphics::prelude::Point {
    fn from(value: &Point) -> Self {
        value.point()
    }
}

impl From<&mut Point> for embedded_graphics::prelude::Point {
    fn from(value: &mut Point) -> Self {
        value.point()
    }
}

impl From<Size> for embedded_graphics::prelude::Size {
    fn from(value: Size) -> Self {
        value.size()
    }
}

impl From<&Size> for embedded_graphics::prelude::Size {
    fn from(value: &Size) -> Self {
        value.size()
    }
}

impl From<&mut Size> for embedded_graphics::prelude::Size {
    fn from(value: &mut Size) -> Self {
        value.size()
    }
}

impl Element {
    pub fn position(&self) -> embedded_graphics::prelude::Point {
        match self {
            Element::Text { position, .. } => position.into(),
            Element::Sprite { position, .. } => position.into(),
            Element::Line { start, .. } => start.into(),
            Element::Polyline { points, .. } => points.first().unwrap_or_default().into(),
            Element::Rectangle { top_left, .. } => top_left.into(),
        }
    }
}

impl Alignment {
    pub fn alignment(&self) -> embedded_graphics::text::Alignment {
        match self {
            Alignment::Left => embedded_graphics::text::Alignment::Left,
            Alignment::Center => embedded_graphics::text::Alignment::Center,
            Alignment::Right => embedded_graphics::text::Alignment::Right,
        }
    }
}
