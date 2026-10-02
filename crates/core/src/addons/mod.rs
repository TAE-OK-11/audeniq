//! Add-on orders share the existing catalog, ACL, audited staff API and durable queues.
pub mod api;
pub mod jobs;
pub mod model;
pub mod workflow;
pub use api::routes;
