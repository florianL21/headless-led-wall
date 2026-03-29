#![no_std]
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;

pub mod flash;
pub mod panel;
// #[macro_use]
pub mod pins;
pub mod resources;
pub mod rest;
pub mod ui;
pub mod wifi;

const DEBUG_DISPLAY: bool = false;

static_toml::static_toml! {
    pub static CONFIG = include_toml!("config.toml");
}
