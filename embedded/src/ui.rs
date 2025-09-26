use core::sync::atomic::Ordering;

use crate::{
    flash::{make_buf, FlashType},
    panel::{FrameBufferExchange, TiledFBType, SYSTEM_IS_UP},
    resources::{bake, get_dino_sprite, get_no_image_sprite, get_wifi_sprite, BakedResource},
    rest::DISPLAY_CONFIG_SIGNAL,
    wifi::{CurrentStateSignal, SystemState},
};
use alloc::{collections::btree_map::BTreeMap, string::String, vec::Vec};
use embassy_executor::task;
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::Drawable;
use embedded_graphics::{geometry::Point, primitives::Line};
use embedded_graphics::{image::Image, primitives::PrimitiveStyleBuilder};
use embedded_graphics::{mono_font::MonoTextStyleBuilder, primitives::Rectangle};
use embedded_graphics::{
    mono_font::{ascii::FONT_5X7, MonoTextStyle},
    primitives::Polyline,
};
use embedded_graphics::{pixelcolor::Rgb888, primitives::PrimitiveStyle};
use embedded_graphics::{prelude::*, primitives::CornerRadiiBuilder};
use embedded_graphics::{primitives::RoundedRectangle, text::Text};
use embedded_layout::{layout::linear::LinearLayout, prelude::*};
use esp_hub75::Color;
use interface::{
    embedded::{string_to_color, CheckedScreenConfig},
    Resource,
};
use interface::{Element, RectangleCorners};
use log::{error, info};
use postcard::from_bytes;

struct SpriteRegister {
    sprites: BTreeMap<String, BakedResource>,
    flash: &'static FlashType,
}

async fn bake_sprite(flash: &FlashType, name: &String) -> Option<BakedResource> {
    let tr = flash.read_transaction().await;
    let mut buf = make_buf();
    info!("Baking sprite {name}...");
    match tr.read(name.as_bytes(), &mut buf).await {
        Ok(len) => match from_bytes::<Resource>(&buf[..len]) {
            Ok(res) => return Some(bake(res)),
            Err(e) => {
                error!("Could not parse '{name}' sprite from flash: {e:?}");
            }
        },
        Err(e) => {
            error!("Failed reading sprite {name} from flash: {e:?}");
        }
    }
    None
}

impl SpriteRegister {
    fn new(flash: &'static FlashType) -> Self {
        Self {
            sprites: BTreeMap::new(),
            flash,
        }
    }

    /// Clear out any sprites which are not in the keep list
    fn clear(&mut self, keep: &[&String]) {
        let keys = self.sprites.keys().cloned().collect::<Vec<_>>();
        for sprite in keys {
            if !keep.contains(&&sprite) {
                self.sprites.remove(&sprite);
            }
        }
    }

    /// Prepare all sprites in the config to be rendered
    async fn prepare(&mut self, keys: &[&String]) {
        for name in keys {
            if !self.sprites.contains_key(*name) {
                if let Some(res) = bake_sprite(self.flash, name).await {
                    self.sprites.insert((**name).clone(), res);
                }
            };
        }
    }

    async fn get_sprite<'a>(&'a mut self, name: &String, now: Instant) -> Option<tinyqoi::Qoi<'a>> {
        let sprite = if self.sprites.contains_key(name) {
            self.sprites.get_mut(name)
        } else {
            None
        };
        sprite?.get_image(now).ok()
    }

    fn needs_redraw(&self, now: Instant) -> bool {
        for (_, sprite) in self.sprites.iter() {
            if sprite.needs_update(now) {
                return true;
            }
        }
        false
    }
}

fn make_primitive_style(
    stroke_color: &Option<String>,
    stroke_width: &Option<u32>,
    fill_color: &Option<String>,
) -> PrimitiveStyle<Color> {
    let mut style = PrimitiveStyleBuilder::new();
    if let Some(color) = stroke_color {
        if let Some(color) = string_to_color(color) {
            style = style.stroke_color(color);
        }
    }
    if let Some(stroke) = stroke_width {
        style = style.stroke_width(*stroke);
    }
    if let Some(fill) = fill_color {
        if let Some(fill) = string_to_color(fill) {
            style = style.fill_color(fill)
        }
    }
    style.build()
}

async fn render_config(
    fb: &mut TiledFBType,
    config: &mut CheckedScreenConfig,
    sprite_register: &mut SpriteRegister,
    err_img: &mut BakedResource,
    now: Instant,
) {
    for element in config.screen.elements.iter_mut() {
        let pos = element.position();
        match element {
            interface::Element::Sprite { name, center, .. } => {
                if let Some(img) = sprite_register.get_sprite(name, now).await {
                    if let Some(point) = center {
                        Image::with_center(&img, point.into()).draw(fb).ok();
                    } else {
                        Image::new(&img, pos).draw(fb).ok();
                    }
                } else if let Ok(img) = err_img.get_image(now) {
                    if let Some(point) = center {
                        Image::with_center(&img, point.into()).draw(fb).ok();
                    } else {
                        Image::new(&img, pos).draw(fb).ok();
                    }
                }
            }
            interface::Element::Text {
                style, text, align, ..
            } => {
                if let Some(style) = config.styles.get(style) {
                    if let Some(align) = align {
                        Text::with_alignment(text, pos, *style, align.alignment())
                            .draw(fb)
                            .ok();
                    } else {
                        Text::new(text, pos, *style).draw(fb).ok();
                    }
                } else {
                    error!("Style {style} not found");
                }
            }
            Element::Line {
                start,
                end,
                color,
                stroke,
            } => {
                let style = make_primitive_style(color, stroke, &None);
                Line::new(start.into(), end.into())
                    .into_styled(style)
                    .draw(fb)
                    .ok();
            }
            Element::Polyline {
                color,
                stroke,
                points,
            } => {
                let style = make_primitive_style(color, stroke, &None);
                let points: Vec<Point> = points.iter().map(|p| p.into()).collect();
                Polyline::new(points.as_slice())
                    .into_styled(style)
                    .draw(fb)
                    .ok();
            }
            Element::Rectangle {
                top_left,
                size,
                fill_color,
                stroke_color,
                stroke,
                rounded_corners,
            } => {
                let style = make_primitive_style(stroke_color, stroke, fill_color);
                let rect = Rectangle::new(top_left.into(), size.into());
                if let Some(corners) = rounded_corners {
                    let corners = match corners {
                        RectangleCorners::Uniform(size) => {
                            CornerRadiiBuilder::new().all(size.into()).build()
                        }
                        RectangleCorners::Different {
                            top_left,
                            top_right,
                            bottom_left,
                            bottom_right,
                        } => {
                            let mut builder = CornerRadiiBuilder::new();
                            if let Some(radius) = top_left {
                                builder = builder.top_left(radius.into());
                            }
                            if let Some(radius) = top_right {
                                builder = builder.top_right(radius.into());
                            }
                            if let Some(radius) = bottom_left {
                                builder = builder.bottom_left(radius.into());
                            }
                            if let Some(radius) = bottom_right {
                                builder = builder.bottom_right(radius.into());
                            }
                            builder.build()
                        }
                    };
                    RoundedRectangle::new(rect, corners)
                        .into_styled(style)
                        .draw(fb)
                        .ok();
                } else {
                    rect.into_styled(style).draw(fb).ok();
                }
            }
        }
    }
}

fn must_redraw(cond: bool, is_dirty: &mut bool, fb: &mut TiledFBType) -> bool {
    if *is_dirty || cond {
        fb.clear(Color::BLACK).ok();
        *is_dirty = true;
        true
    } else {
        false
    }
}

fn draw_connect_screen(
    fb: &mut TiledFBType,
    text_style: MonoTextStyle<'_, Color>,
    display_area: Rectangle,
    wifi: &mut BakedResource,
    now: Instant,
    needs_render: &mut bool,
    message: &str,
) {
    if must_redraw(wifi.needs_update(now), needs_render, fb) {
        if let Ok(img) = wifi.get_image(now) {
            LinearLayout::vertical(
                Chain::new(Image::new(&img, Point::zero())).append(Text::new(
                    message,
                    Point::zero(),
                    text_style,
                )),
            )
            .with_alignment(horizontal::Center)
            .arrange()
            .align_to(&display_area, horizontal::Center, vertical::Center)
            .draw(fb)
            .ok();
        }
    }
}

#[task]
pub async fn display_task(
    rx: &'static FrameBufferExchange,
    tx: &'static FrameBufferExchange,
    mut fb: &'static mut TiledFBType,
    wifi_up: &'static CurrentStateSignal,
    flash: &'static FlashType,
) {
    info!("display_task: starting!");

    let mut wifi = get_wifi_sprite();
    let mut dino = get_dino_sprite();
    let mut err_img = get_no_image_sprite();

    let mut wifi_state = SystemState::WIFIConnecting;

    let wifi_text_style = MonoTextStyleBuilder::new()
        .font(&FONT_5X7)
        .text_color(Rgb888::YELLOW)
        .build();

    let display_area = fb.bounding_box();

    let mut display_config = None;
    let mut sprite_register = SpriteRegister::new(flash);
    let mut needs_render = true;

    loop {
        if wifi_up.signaled() {
            wifi_state = wifi_up.wait().await;
            needs_render = true;
        }
        let now = Instant::now();
        match wifi_state {
            SystemState::Ready | SystemState::WIFIConnected => {
                SYSTEM_IS_UP.store(true, Ordering::Relaxed);
                if DISPLAY_CONFIG_SIGNAL.signaled() {
                    display_config = DISPLAY_CONFIG_SIGNAL.wait().await;
                    if let Some(ref conf) = display_config {
                        let keep: Vec<_> = conf
                            .screen
                            .elements
                            .iter()
                            .filter_map(|e| {
                                if let Element::Sprite { name, .. } = e {
                                    Some(name)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        sprite_register.clear(keep.as_slice());
                        sprite_register.prepare(keep.as_slice()).await;
                    } else {
                        sprite_register.clear(&[]);
                    }
                    needs_render = true;
                }
                if let Some(ref mut conf) = display_config {
                    if must_redraw(sprite_register.needs_redraw(now), &mut needs_render, fb) {
                        render_config(fb, conf, &mut sprite_register, &mut err_img, now).await;
                    }
                } else if must_redraw(dino.needs_update(now), &mut needs_render, fb) {
                    if let Ok(img) = dino.get_image(now) {
                        Image::new(&img, Point::zero()).draw(fb).ok();
                    }
                }
            }
            SystemState::WIFIConnecting => {
                SYSTEM_IS_UP.store(false, Ordering::Relaxed);
                draw_connect_screen(
                    fb,
                    wifi_text_style,
                    display_area,
                    &mut wifi,
                    now,
                    &mut needs_render,
                    "Connecting to WIFI",
                );
            }
            SystemState::Disconnected => {
                SYSTEM_IS_UP.store(false, Ordering::Relaxed);
                draw_connect_screen(
                    fb,
                    wifi_text_style,
                    display_area,
                    &mut wifi,
                    now,
                    &mut needs_render,
                    "Lost WIFI...",
                );
            }
            SystemState::Failed => {
                SYSTEM_IS_UP.store(false, Ordering::Relaxed);
                draw_connect_screen(
                    fb,
                    wifi_text_style,
                    display_area,
                    &mut wifi,
                    now,
                    &mut needs_render,
                    "Failed to connect. Retrying...",
                );
            }
            SystemState::WIFIWaitForIP => {
                SYSTEM_IS_UP.store(false, Ordering::Relaxed);
                draw_connect_screen(
                    fb,
                    wifi_text_style,
                    display_area,
                    &mut wifi,
                    now,
                    &mut needs_render,
                    "Waiting for IP",
                );
            }
        }
        // only exchange the framebuffers if there is something new to render
        if needs_render {
            needs_render = false;
            // send the frame buffer to be rendered
            tx.signal(fb);
            // get the next frame buffer
            fb = rx.wait().await;
        } else {
            // give other tasks some time to run as well
            Timer::after(Duration::from_millis(30)).await;
        }
    }
}
