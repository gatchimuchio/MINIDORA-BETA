//! Independently versioned Rust successor development implementation.
//! Core alone binds goals, adopts content, mutates memory and reconciles effects.
#![forbid(unsafe_code)]
mod asr_julius;
mod attached_document;
mod attachment_analysis;
mod branch_text;
mod capabilities;
mod clarification;
pub mod code_capability;
pub mod code_generation;
mod code_intent;
mod conversation_history_input;
mod conversation_input;
pub mod core;
mod counterfactual;
mod counterfactual_input;
mod csv_chart;
mod csv_chart_context;
mod csv_wasm;
mod decision;
mod decision_input;
mod decision_text;
pub mod document;
mod durability;
mod fiction;
mod fiction_input;
mod hearing;
mod image_artifact;
mod image_delivery;
mod image_ocr;
pub mod input;
pub mod language;
pub mod math;
pub mod meaning_compare;
mod memory_merge;
mod message_revision;
mod news;
mod news_input;
mod news_source;
mod ocr_worker;
pub mod output;
#[cfg(windows)]
mod pdf_raster;
mod pdf_scan;
#[cfg(windows)]
mod pdf_text;
mod pdf_worker;
pub mod ports;
mod premise_correction_input;
mod purpose_profile;
pub mod recovery;
pub mod relations;
mod research_goal;
pub mod scheduler;
pub mod search;
mod speech;
mod speech_input;
pub mod store;
pub mod text_editing;
pub mod text_summary;
mod translation;
mod translation_input;
mod translation_lexicon;
mod translation_time;
mod tts_japanese;
mod wasm_execution;
mod wasm_process;
mod web_answer;
mod web_research;
mod work_plan;
mod work_plan_input;
mod writing;
mod xlsx;
pub use asr_julius::configure_japanese_recognition;
pub use capabilities::{
    CapabilityPermission, EngineCapabilities, EngineCapability, EngineCaseStatus,
    EngineResultStatus, NATIVE_API_VERSION,
};
pub use core::Engine;
pub use durability::{snapshot_local_attachment, snapshot_local_csv_attachment};
pub use ocr_worker::configure_image_ocr;
#[cfg(windows)]
pub use pdf_worker::{configure_pdf_worker, run_pdf_worker_stdio};
pub use speech_input::RecognizedWave;
pub use tts_japanese::configure_japanese_speech;
