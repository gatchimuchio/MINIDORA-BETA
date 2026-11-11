//! Independently versioned Rust successor development implementation.
//! Core alone binds goals, adopts content, mutates memory and reconciles effects.
#![forbid(unsafe_code)]
pub mod code_capability;
pub mod code_generation;
pub mod core;
pub mod document;
pub mod input;
pub mod language;
pub mod math;
pub mod meaning_compare;
pub mod output;
pub mod ports;
pub mod recovery;
pub mod relations;
pub mod scheduler;
pub mod search;
pub mod store;
pub use core::Engine;
