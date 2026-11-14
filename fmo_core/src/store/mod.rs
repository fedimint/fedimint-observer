//! Storage: PostgreSQL schema and the writers and row types shared across
//! layers. Collectors and processors write through these; the read queries of
//! each endpoint stay next to its handler.
//!
//! - `schema.rs`: core migrations, module schemas and migrations, and the
//!   refresh of materialized views
//! - `rows.rs`: core's rows of a session's items, written by ingest and by
//!   module projectors
//! - `db.rs`: row types of `federations` and `sessions`
//! - `observations.rs`: per-federation last-known-good snapshots
//!   (`federation_observations`)

pub mod db;
mod observations;
pub(crate) mod rows;
pub(crate) mod schema;
