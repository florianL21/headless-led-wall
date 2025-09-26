#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(impl_trait_in_bindings)]
#![feature(associated_type_defaults)]
#![feature(new_zeroed_alloc)]

extern crate alloc;

pub mod flash;
pub mod panel;
pub mod resources;
pub mod rest;
pub mod ui;
pub mod wifi;

static_toml::static_toml! {
    pub static CONFIG = include_toml!("config.toml");
}
