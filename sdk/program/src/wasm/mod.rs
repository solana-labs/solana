//! solana-program Javascript interface
#![cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub mod pubkey;

/// Initialize Javascript logging and panic handler
#[wasm_bindgen]
pub fn init() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_log::init_with_level(log::Level::Info).unwrap();
}

pub fn display_to_jsvalue<T: std::fmt::Display>(display: T) -> JsValue {
    display.to_string().into()
}
