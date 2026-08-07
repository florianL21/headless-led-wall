use core::sync::atomic::Ordering;

use crate::{
    DEBUG_DISPLAY,
    flash::{FlashType, make_buf},
    panel::{DisplayFB, PANEL_ON, SYSTEM_IS_UP},
    resources::{BakedResource, bake, get_dino_sprite, get_no_image_sprite, get_wifi_sprite},
    rest::DISPLAY_CONFIG_SIGNAL,
    wifi::{CurrentStateSignal, SystemState},
};
use alloc::{boxed::Box, collections::btree_map::BTreeMap, format, string::String, vec::Vec};
use average::{Estimate, Variance};
use embassy_time::{Duration, Instant};
use embedded_graphics::Drawable;
use embedded_graphics::{geometry::Point, primitives::Line};
use embedded_graphics::{image::Image, primitives::PrimitiveStyleBuilder};
use embedded_graphics::{mono_font::MonoTextStyleBuilder, primitives::Rectangle};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_5X7},
    primitives::Polyline,
};
use embedded_graphics::{pixelcolor::Rgb888, primitives::PrimitiveStyle};
use embedded_graphics::{prelude::*, primitives::CornerRadiiBuilder};
use embedded_graphics::{primitives::RoundedRectangle, text::Text};
use embedded_layout::{layout::linear::LinearLayout, prelude::*};
use esp_hub75::Color;
use interface::{
    Element, Overlay, RectangleCorners,
    embedded::{BuiltTextStyles, CheckedScreenConfig, ScrollAnimationInstance},
};
use interface::{Resource, embedded::string_to_color};
use log::{error, info};
use postcard::from_bytes;

const LOG_INTERVAL: Duration = Duration::from_secs(5);

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
            if !self.sprites.contains_key(*name)
                && let Some(res) = bake_sprite(self.flash, name).await
            {
                self.sprites.insert((**name).clone(), res);
            };
        }
    }

    fn get_sprite<'a>(&'a mut self, name: &String, now: Instant) -> Option<tinyqoi::Qoi<'a>> {
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
    if let Some(color) = stroke_color
        && let Some(color) = string_to_color(color)
    {
        style = style.stroke_color(color);
    }
    if let Some(stroke) = stroke_width {
        style = style.stroke_width(*stroke);
    }
    if let Some(fill) = fill_color
        && let Some(fill) = string_to_color(fill)
    {
        style = style.fill_color(fill)
    }
    style.build()
}

async fn render_config(
    fb: &mut DisplayFB,
    elements: &Vec<Element>,
    styles: &BuiltTextStyles,
    sprite_register: &mut SpriteRegister,
    err_img: &mut BakedResource,
    now: Instant,
    offset: Point,
) {
    for element in elements.iter() {
        let pos = element.position() - offset;
        match element {
            interface::Element::Sprite { name, center, .. } => {
                if let Some(img) = sprite_register.get_sprite(name, now) {
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
                if let Some(style) = styles.get(style) {
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
                Line::new(start.point() - offset, end.point() - offset)
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
                let points: Vec<Point> = points.iter().map(|p| p.point() - offset).collect();
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
                let rect = Rectangle::new(top_left.point() - offset, size.into());
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

fn must_redraw(cond: bool, is_dirty: &mut bool, fb: &mut DisplayFB) -> bool {
    if *is_dirty || cond {
        fb.clear(Color::BLACK).ok();
        *is_dirty = true;
        true
    } else {
        false
    }
}

fn draw_connect_screen(
    fb: &mut DisplayFB,
    text_style: MonoTextStyle<'_, Color>,
    display_area: Rectangle,
    wifi: &mut BakedResource,
    now: Instant,
    needs_render: &mut bool,
    message: String,
) {
    if must_redraw(wifi.needs_update(now), needs_render, fb)
        && let Ok(img) = wifi.get_image(now)
    {
        LinearLayout::vertical(
            Chain::new(Image::new(&img, Point::zero())).append(Text::new(
                message.as_str(),
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

struct ScrollAnimationState {
    anim: ScrollAnimationInstance,
    last_update: Instant,
    current_offset: Point,
    iterator: Box<dyn Iterator<Item = Point>>,
    counter: usize,
    anim_len: usize,
}

impl ScrollAnimationState {
    fn new(anim: ScrollAnimationInstance) -> Self {
        let mut iter = anim.iter();
        let point = iter
            .next()
            .expect("ScrollAnimation must yield at least one point");
        let anim_len = anim.len();
        ScrollAnimationState {
            anim,
            last_update: Instant::now(),
            iterator: iter,
            current_offset: point,
            counter: 0,
            anim_len,
        }
    }

    fn current_offset(&self) -> Point {
        self.current_offset
    }

    fn needs_update(&mut self) -> bool {
        if self.anim.needs_redraw(self.last_update) {
            self.last_update = Instant::now();
            self.current_offset = self.iterator.next().expect("ScrollAnimation iterator must never exhaust. Check your implementation and potentially add a .cycle() call to it");
            self.counter += 1;
            if self.counter >= self.anim_len {
                self.counter = 0;
            }
            true
        } else {
            false
        }
    }

    fn is_finished(&self) -> bool {
        self.counter >= self.anim_len.saturating_sub(1)
    }
}

pub struct Renderer {
    wifi: BakedResource,
    dino: BakedResource,
    err_img: BakedResource,
    wifi_state: SystemState,
    wifi_text_style: MonoTextStyle<'static, Rgb888>,
    display_area: Rectangle,
    new_display_config: Option<CheckedScreenConfig>,
    display_config: Option<CheckedScreenConfig>,
    sprite_register: SpriteRegister,
    needs_render: bool,
    current_animation: ScrollAnimationState,
    render_time: Variance,
    last_log: Instant,
    force_refresh: bool,
    wifi_up: &'static CurrentStateSignal,
}

impl Renderer {
    pub fn new(
        wifi_up: &'static CurrentStateSignal,
        flash: &'static FlashType,
        display_area: Rectangle,
    ) -> Self {
        Self {
            wifi: get_wifi_sprite(),
            dino: get_dino_sprite(),
            err_img: get_no_image_sprite(),
            wifi_state: SystemState::WIFIConnecting,
            wifi_text_style: MonoTextStyleBuilder::new()
                .font(&FONT_5X7)
                .text_color(Rgb888::YELLOW)
                .build(),
            display_area,
            new_display_config: None,
            display_config: None,
            sprite_register: SpriteRegister::new(flash),
            needs_render: true,
            current_animation: ScrollAnimationState::new(ScrollAnimationInstance::still()),
            render_time: Variance::new(),
            last_log: Instant::now(),
            force_refresh: false,
            wifi_up,
        }
    }

    pub async fn render(&mut self, fb: &mut DisplayFB) -> bool {
        let mut has_rendered = false;
        if self.wifi_up.signaled() {
            self.wifi_state = self.wifi_up.wait().await;
            self.needs_render = true;
        }
        let now = Instant::now();
        let connect_message: Option<String> = match self.wifi_state {
            SystemState::WIFIConnecting => Some("Connecting to WIFI".into()),
            SystemState::Disconnected => Some("Lost WIFI...".into()),
            SystemState::Failed(e) => match e {
                esp_radio::wifi::WifiError::Unsupported
                | esp_radio::wifi::WifiError::Failed
                | esp_radio::wifi::WifiError::InvalidPassword
                | esp_radio::wifi::WifiError::InvalidArguments
                | esp_radio::wifi::WifiError::InvalidSsid
                | esp_radio::wifi::WifiError::OutOfMemory => {
                    Some(format!("Failed to connect ({:?}).\nRetrying...", e))
                }
                esp_radio::wifi::WifiError::Disconnected(di) => {
                    Some(format!("Lost WIFI ({:?}).\nRetrying...", di.reason))
                }
                _ => Some("Failed to connect (Reason unknown).\nRetrying...".into()),
            },
            SystemState::WIFIWaitForIP => Some("Waiting for IP".into()),
            SystemState::Ready | SystemState::WIFIConnected => None,
        };
        if let Some(msg) = connect_message {
            SYSTEM_IS_UP.store(false, Ordering::Relaxed);
            draw_connect_screen(
                fb,
                self.wifi_text_style,
                self.display_area,
                &mut self.wifi,
                now,
                &mut self.needs_render,
                msg,
            );
        } else {
            SYSTEM_IS_UP.store(true, Ordering::Relaxed);
            if let Some(conf) = DISPLAY_CONFIG_SIGNAL.try_take() {
                info!("got new config!");
                self.new_display_config = conf;
                if self.new_display_config.is_none() {
                    self.sprite_register.clear(&[]);
                }
            }
            // only change configuration if the scroll animation is finished or the panel is currently off
            if self.new_display_config.is_some()
                && (self.current_animation.is_finished() || self.force_refresh)
            {
                info!("display new config!");
                self.display_config = self.new_display_config.take();
                if let Some(ref conf) = self.display_config {
                    let default_overlay = Overlay::default();
                    let overlay = conf.overlay.as_ref().unwrap_or(&default_overlay);
                    let keep: Vec<_> = conf
                        .screen
                        .elements
                        .iter()
                        .chain(overlay.elements.iter())
                        .filter_map(|e| {
                            if let Element::Sprite { name, .. } = e {
                                Some(name)
                            } else {
                                None
                            }
                        })
                        .collect();
                    self.sprite_register.clear(keep.as_slice());
                    self.sprite_register.prepare(keep.as_slice()).await;
                    self.current_animation =
                        ScrollAnimationState::new(conf.screen.scroll_animation.clone().instance(
                            conf.screen.screen_size.clone(),
                            conf.screen.canvas_size.clone(),
                        ));
                    self.needs_render = true;
                }
            }
            if let Some(ref mut conf) = self.display_config {
                if must_redraw(
                    self.sprite_register.needs_redraw(now) || self.current_animation.needs_update(),
                    &mut self.needs_render,
                    fb,
                ) {
                    render_config(
                        fb,
                        &conf.screen.elements,
                        &conf.styles,
                        &mut self.sprite_register,
                        &mut self.err_img,
                        now,
                        self.current_animation.current_offset(),
                    )
                    .await;
                    if let Some(overlay) = &conf.overlay {
                        render_config(
                            fb,
                            &overlay.elements,
                            &conf.styles,
                            &mut self.sprite_register,
                            &mut self.err_img,
                            now,
                            Point::zero(),
                        )
                        .await;
                    }
                }
            } else if must_redraw(self.dino.needs_update(now), &mut self.needs_render, fb)
                && let Ok(img) = self.dino.get_image(now)
            {
                Image::new(&img, Point::zero()).draw(fb).ok();
            }
        }
        self.render_time.add(now.elapsed().as_micros() as f64);
        // only exchange the framebuffers if there is something new to render
        if self.needs_render {
            has_rendered = true;
            self.needs_render = false;
            // If panel is off at this point we need to force a config refresh next time we get a new FB
            self.force_refresh = !PANEL_ON.load(Ordering::Relaxed);
        }

        if DEBUG_DISPLAY && self.last_log.elapsed() > LOG_INTERVAL {
            info!(
                "render time: {:.2} us ± {:.2}",
                self.render_time.mean(),
                self.render_time.error()
            );
            self.render_time = Variance::new();
            self.last_log = Instant::now();
        }
        has_rendered
    }
}
