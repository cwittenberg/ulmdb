use serde_json::{Value, json};
use std::{fs, os::unix::fs::MetadataExt, path::Path};
#[derive(Default)]
pub struct Report {
    pub samples: Vec<Value>,
    pub lifecycle: Vec<Value>,
    pub memory: Vec<Value>,
}
impl Report {
    pub fn rss(&mut self, phase: &str) {
        let content = fs::read_to_string("/proc/self/status").unwrap();
        let mut row = json!({"phase":phase});
        for name in [
            "VmRSS", "VmHWM", "VmSize", "RssAnon", "RssFile", "RssShmem", "VmPTE",
        ] {
            let n = content
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{name}:")))
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse::<u64>().ok());
            row[format!("{}_kib", name.to_ascii_lowercase())] = json!(n);
        }
        self.memory.push(row);
    }
    pub fn life(&mut self, phase: &str, elapsed_ns: u128, details: Value) {
        self.lifecycle
            .push(json!({"phase":phase,"elapsed_ns":elapsed_ns,"details":details}));
    }
}
pub fn file_bytes(path: &Path) -> Value {
    fn sum(path: &Path) -> (u64, u64) {
        let mut total = (0, 0);
        for e in fs::read_dir(path).unwrap() {
            let p = e.unwrap().path();
            let (apparent, allocated) = if p.is_dir() {
                sum(&p)
            } else {
                let m = p.metadata().unwrap();
                (m.len(), m.blocks() * 512)
            };
            total.0 += apparent;
            total.1 += allocated;
        }
        total
    }
    let (apparent, allocated) = sum(path);
    json!({"apparent_bytes":apparent,"allocated_bytes":allocated})
}
