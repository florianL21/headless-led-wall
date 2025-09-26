#![no_std]

extern crate alloc;
use crate::alloc::string::ToString;
use alloc::collections::btree_map::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "server")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(feature = "server")]
extern crate std;

#[cfg(not(feature = "server"))]
pub mod embedded;

pub type GlobalStylesType = BTreeMap<String, TextStyle>;

#[cfg(feature = "server")]
const COLOR_HASH_REGEX: &str = r"^[0-9a-fA-F]{6}$";

#[derive(Deserialize, Debug, PartialEq)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
pub struct Point {
    /// X position of the point
    #[cfg_attr(feature = "server", schemars(range(min = 0, max = 192)))]
    pub x: i32,
    /// Y position of the point
    #[cfg_attr(feature = "server", schemars(range(min = 0, max = 96)))]
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            width: w,
            height: h,
        }
    }

    pub fn zero() -> Self {
        Self {
            width: 0,
            height: 0,
        }
    }
}

#[derive(Deserialize, Debug, PartialEq)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
pub enum Alignment {
    Left,
    Center,
    Right,
}

#[derive(Deserialize, Debug, PartialEq)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
// #[serde(deny_unknown_fields, tag = "kind")]
pub enum Element {
    /// Display a text element at the given position
    Text {
        /// One of the styles from the text_styles map
        style: String,
        /// The text that should be displayed
        text: String,
        /// Position of the text. If not specified will be 0,0.
        /// This is useful if this item is added nested in a layout
        position: Point,
        /// How to align the text
        align: Option<Alignment>,
    },
    /// Display a sprite at the given position
    Sprite {
        /// Position of the sprite. If not specified will be 0,0.
        /// This is useful if this item is added nested in a layout
        position: Point,
        /// Name of the sprite. Must exist in the sprite directory. Does not include the file extension.
        name: String,
        /// Center the sprite around a given point
        center: Option<Point>,
    },
    /// Draw a line
    Line {
        /// Start of the line
        start: Point,
        /// End of the line
        end: Point,
        /// Color of the line
        color: Option<String>,
        /// Width of the line
        stroke: Option<u32>,
    },
    Polyline {
        /// Points of the polyline
        points: Vec<Point>,
        /// Color of the line
        color: Option<String>,
        /// Width of the line
        stroke: Option<u32>,
    },
    Rectangle {
        /// top left position of the rectangle
        top_left: Point,
        /// width of the rectangle
        size: Size,
        /// Fill color
        fill_color: Option<String>,
        /// Color of the rectangle stroke
        stroke_color: Option<String>,
        /// Stroke width of the rectangles stroke
        stroke: Option<u32>,
        /// Corner radi of a rounded rectangle
        rounded_corners: Option<RectangleCorners>,
    },
}

#[derive(Deserialize, Debug, PartialEq)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
pub enum RectangleCorners {
    Uniform(Size),
    Different {
        top_left: Option<Size>,
        top_right: Option<Size>,
        bottom_left: Option<Size>,
        bottom_right: Option<Size>,
    },
}

impl RectangleCorners {
    pub fn new() -> Self {
        Self::Different {
            top_left: None,
            top_right: None,
            bottom_left: None,
            bottom_right: None,
        }
    }

    pub fn uniform(size: Size) -> Self {
        Self::Different {
            top_left: Some(size.clone()),
            top_right: Some(size.clone()),
            bottom_left: Some(size.clone()),
            bottom_right: Some(size),
        }
    }

    fn expand(self) -> Self {
        if let RectangleCorners::Uniform(s) = self {
            Self::Different {
                top_left: Some(s.clone()),
                top_right: Some(s.clone()),
                bottom_left: Some(s.clone()),
                bottom_right: Some(s),
            }
        } else {
            self
        }
    }

    pub fn top_right(mut self, size: Size) -> Self {
        self = self.expand();
        if let RectangleCorners::Different {
            ref mut top_right, ..
        } = self
        {
            *top_right = Some(size);
        }
        self
    }

    pub fn top_left(mut self, size: Size) -> Self {
        self = self.expand();
        if let RectangleCorners::Different {
            ref mut top_left, ..
        } = self
        {
            *top_left = Some(size);
        }
        self
    }

    pub fn bottom_left(mut self, size: Size) -> Self {
        self = self.expand();
        if let RectangleCorners::Different {
            ref mut bottom_left,
            ..
        } = self
        {
            *bottom_left = Some(size);
        }
        self
    }

    pub fn bottom_right(mut self, size: Size) -> Self {
        self = self.expand();
        if let RectangleCorners::Different {
            ref mut bottom_right,
            ..
        } = self
        {
            *bottom_right = Some(size);
        }
        self
    }
}

impl Element {
    pub fn new_text(style: &str, text: String, position: Point) -> Self {
        Self::Text {
            style: style.to_string(),
            text,
            position,
            align: None,
        }
    }

    pub fn new_sprite(name: String, position: Point) -> Self {
        Self::Sprite {
            name,
            position,
            center: None,
        }
    }

    pub fn new_line(start: Point, end: Point, color: &str) -> Self {
        Self::Line {
            start,
            end,
            color: Some(color.to_string()),
            stroke: Some(1),
        }
    }

    pub fn new_polyline(points: Vec<Point>, color: &str) -> Self {
        Self::Polyline {
            points,
            color: Some(color.to_string()),
            stroke: Some(1),
        }
    }

    pub fn new_rect(left_top: Point, size: Size) -> Self {
        Self::Rectangle {
            top_left: left_top,
            size: size,
            fill_color: None,
            stroke_color: None,
            stroke: None,
            rounded_corners: None,
        }
    }

    /// Only applicable to lines and rect
    pub fn with_stroke(mut self, stroke_width: u32) -> Self {
        match self {
            Element::Line { ref mut stroke, .. } => {
                *stroke = Some(stroke_width);
            }
            Element::Polyline { ref mut stroke, .. } => *stroke = Some(stroke_width),
            Element::Rectangle { ref mut stroke, .. } => *stroke = Some(stroke_width),
            _ => {}
        }
        self
    }

    /// Only applicable to lines and rect
    pub fn stroke_color(mut self, stroke_color: &str) -> Self {
        match self {
            Element::Line { ref mut color, .. } => *color = Some(stroke_color.into()),
            Element::Polyline { ref mut color, .. } => *color = Some(stroke_color.into()),
            Element::Rectangle {
                stroke_color: ref mut color,
                ..
            } => *color = Some(stroke_color.into()),
            _ => {}
        }
        self
    }

    /// Only applicable to rectangles
    pub fn fill_color(mut self, fill_color: &str) -> Self {
        match self {
            Element::Rectangle {
                fill_color: ref mut color,
                ..
            } => *color = Some(fill_color.into()),
            _ => {}
        }
        self
    }

    /// Only applicable to rectangles
    /// Adds the same corner radius to all corners of a rects
    pub fn with_rounded_corners(mut self, corners: Size) -> Self {
        match self {
            Element::Rectangle {
                ref mut rounded_corners,
                ..
            } => *rounded_corners = Some(RectangleCorners::Uniform(corners)),
            _ => {}
        }
        self
    }

    /// Only applicable to rectangles
    /// Adds corner configuration to a rectangle
    pub fn with_corners(mut self, corners: RectangleCorners) -> Self {
        match self {
            Element::Rectangle {
                ref mut rounded_corners,
                ..
            } => *rounded_corners = Some(corners),
            _ => {}
        }
        self
    }

    /// Only applicable to sprites
    pub fn centered(mut self, centerpoint: Point) -> Self {
        match self {
            Element::Sprite { ref mut center, .. } => {
                *center = Some(centerpoint);
            }
            _ => {}
        }
        self
    }

    /// Only applicable to text
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        match self {
            Element::Text { ref mut align, .. } => {
                *align = Some(alignment);
            }
            _ => {}
        }
        self
    }
}

#[derive(Deserialize, Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
pub enum FontName {
    Font4X6,
    Font5X7,
    Font5X8,
    Font6X9,
    Font6X10,
    Font6X12,
    Font6X13,
    Font6X13Bold,
    Font6X13Italic,
    Font7X13,
    Font7X13Bold,
    Font7X13Italic,
    Font7X14,
    Font7X14Bold,
    Font8X13,
    Font8X13Bold,
    Font8X13Italic,
    Font9X15,
    Font9X15Bold,
    Font9X18,
    Font9X18Bold,
    Font10X20,
    Profont7,
    Profont9,
    Profont10,
    Profont12,
    Profont14,
    Profont18,
    Profont24,
}

#[derive(Deserialize, Debug, PartialEq)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    /// Foreground color of the text
    #[cfg_attr(feature = "server", schemars(regex(pattern = COLOR_HASH_REGEX)))]
    pub text_color: String,
    /// Font to use for the text
    pub font: FontName,
    /// Background color of the font
    #[cfg_attr(feature = "server", schemars(regex(pattern = COLOR_HASH_REGEX)))]
    pub background_color: Option<String>,
    /// Wether to underline the text or not
    pub underline: Option<bool>,
    /// Wether to strikethrough the text or not
    pub strikethrough: Option<bool>,
}

impl TextStyle {
    pub fn new(text_color: &str, font: FontName) -> Self {
        Self {
            text_color: text_color.to_string(),
            font,
            background_color: None,
            strikethrough: None,
            underline: None,
        }
    }

    pub fn with_background(mut self, background_color: String) -> Self {
        self.background_color = Some(background_color);
        self
    }

    pub fn with_underline(mut self, underline: bool) -> Self {
        self.underline = Some(underline);
        self
    }
}

#[derive(Deserialize, Debug, PartialEq)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Screen {
    /// Array of elements to display on the screen
    pub elements: Vec<Element>,
}

impl Screen {
    pub fn new(elements: Vec<Element>) -> Self {
        Self { elements }
    }
}

#[derive(Deserialize, Debug, PartialEq)]
#[cfg_attr(feature = "server", derive(Serialize, JsonSchema))]
pub struct Configuration {
    /// Array of screens to display. For now only the first screen is acutally read.
    pub screens: Vec<Screen>,
    /// Map of text styles
    pub text_styles: GlobalStylesType,
}

impl Configuration {
    pub fn new(screens: Vec<Screen>) -> Self {
        Self {
            screens,
            text_styles: GlobalStylesType::new(),
        }
    }

    pub fn add_style(mut self, name: &str, style: TextStyle) -> Self {
        self.text_styles.insert(name.to_string(), style);
        self
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Resource {
    pub frames: Vec<Vec<u8>>,
    pub frame_time_ms: u16,
}

impl Resource {
    pub fn new(frames: Vec<Vec<u8>>, frame_time_ms: u16) -> Self {
        Self {
            frames,
            frame_time_ms,
        }
    }
}
