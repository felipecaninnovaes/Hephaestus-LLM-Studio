//! Handlers HTTP do manager organizados por domínio (MM-06).

pub mod generations;
pub mod health;
pub mod jobs;
pub mod models;
pub mod nodes;

pub use generations::*;
pub use health::*;
pub use jobs::*;
pub use models::*;
pub use nodes::*;
