//! Full shared-work benchmark. Native LMDB is a benchmark-only dependency.
pub mod backend;
pub mod cold;
pub mod dataset;
pub mod durable;
pub mod harness;
pub mod native;
pub mod oracle;
pub mod report;
pub mod run;
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
