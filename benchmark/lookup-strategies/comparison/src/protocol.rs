use crate::data::{Config, Record};
use serde_json::Value;
use std::ops::Bound;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn context(error: impl std::fmt::Display, phase: &str) -> Box<dyn std::error::Error> {
    format!("{phase}: {error}").into()
}

pub trait Lookup {
    fn get(&self, key: &[u8]) -> Result<Option<&[u8]>>;
    fn visit(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
        limit: usize,
        callback: impl FnMut(&[u8], &[u8]),
    ) -> Result<()>;
    fn stats(&self) -> Value {
        serde_json::json!({"algorithm":"native", "index_bytes":0})
    }
}
pub trait Snapshot {
    type Reader<'a>: Lookup
    where
        Self: 'a;
    fn prepare(&self, config: &Config) -> Result<Self::Reader<'_>>;
    fn identity(&self) -> Value;
}
pub trait Engine {
    fn before_pin(&self) -> Result<Value> {
        Ok(serde_json::json!({"maintenance":"none"}))
    }

    fn diagnostics(&self) -> Value {
        serde_json::json!({})
    }

    type Read<'a>: Snapshot
    where
        Self: 'a;
    fn snapshot(&self) -> Result<Self::Read<'_>>;
    fn apply(&self, rows: &[Record], changes: &[(usize, [u8; 32])]) -> Result<()>;
}
