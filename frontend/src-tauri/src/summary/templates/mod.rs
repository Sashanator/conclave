//! Meeting summary templates.
//!
//! Templates live as JSON files in the user's folder (`%APPDATA%\Conclave\templates\`).
//! The shipped templates are seeds: copied there once by `seed_templates`, then owned by the user.
//! The Standard template is also embedded in the binary, so summaries still work if its file is gone.

mod defaults;
mod store;
mod types;

pub use store::*;
pub use types::{Template, TemplateSection};
