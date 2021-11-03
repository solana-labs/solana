#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)] // TODO: Remove

pub mod instruction_processor;
pub mod instruction_recorder;
pub mod invoke_context;
pub mod log_collector;
pub mod native_loader;
