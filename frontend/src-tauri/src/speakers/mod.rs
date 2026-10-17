pub mod sentences;
pub mod cluster;
pub mod diarize;
pub mod features;
pub mod levels;
pub mod models;
pub mod runtime;
pub mod settings;

/// The feature runs only when it is switched on and its models are present.
pub fn is_active() -> bool {
    settings::is_enabled() && models::models_ready()
}
