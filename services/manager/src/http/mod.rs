//! Camada HTTP do manager (extractors, middleware, routes, handlers, state).

pub mod extract;
pub mod handlers;
pub mod middleware;
pub mod routes;
pub mod state;

pub use extract::*;
pub use handlers::*;
pub use middleware::*;
pub use routes::*;
pub use state::*;
