//! Access to Linear's GraphQL API: the HTTP client and reusable query text.

pub mod client;
mod queries;

pub use client::LinearClient;
