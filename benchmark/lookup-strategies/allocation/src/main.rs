#![forbid(unsafe_code)]
mod source;
use serde_json::{Value, json};
use source::Result;
use std::{
    io::{self, Write},
    path::PathBuf,
    time::Instant,
};
use ulmdb::lookup::{HashLimits, Options};

struct Config {
    keys: usize,
    directory: PathBuf,
    index_budget: usize,
}
fn config() -> Result<Config> {
    let mut c = Config {
        keys: 2_000_000,
        directory: PathBuf::new(),
        index_budget: 512 << 20,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let v = args.next().ok_or("every option needs a value")?;
        match flag.as_str() {
            "--keys" => c.keys = v.parse()?,
            "--dir" => c.directory = v.into(),
            "--index-budget-bytes" => c.index_budget = v.parse()?,
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    if c.keys == 0
        || c.keys > 4_000_000
        || c.directory.as_os_str().is_empty()
        || c.directory.exists()
    {
        return Err("--keys must be1..4M and --dir must be fresh".into());
    }
    Ok(c)
}
fn send(value: Value) -> Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
fn command(expected: &str) -> Result<()> {
    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 || line.trim() != expected {
        return Err(format!("expected controller command {expected}").into());
    }
    Ok(())
}
fn memory() -> Result<Value> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let mut out = json!({});
    for name in ["VmSize", "VmRSS", "VmHWM", "VmData"] {
        let number = status
            .lines()
            .find_map(|s| s.strip_prefix(&format!("{name}:")))
            .and_then(|s| s.split_whitespace().next())
            .ok_or("missing process memory field")?
            .parse::<u64>()?;
        out[name] = json!(number * 1024);
    }
    Ok(out)
}
fn run(c: &Config) -> Result<()> {
    let required = ulmdb::lookup::hash_index_bytes(c.keys)?;
    if required > c.index_budget {
        return Err("configured index budget would reject before allocator; invalid probe".into());
    }
    let started = Instant::now();
    let env = source::create(&c.directory, c.keys)?;
    let read = env.begin_read();
    let sequence = read.sequence();
    let commit = read.commit_identity();
    let original_native = read.kv(b"kv")?;
    let first_key = 0u64.to_be_bytes();
    let original_value = original_native
        .get(&first_key)?
        .ok_or("original native first key missing")?;
    source::verify(&read, c.keys)?;
    send(
        json!({"type":"ready","schema":1,"keys":c.keys,"required_index_bytes":required,
        "index_budget_bytes":c.index_budget,"max_entries":c.keys,"max_value_bytes":source::VALUE_BYTES,
        "max_total_value_bytes":c.keys*source::VALUE_BYTES,"value_bytes":source::VALUE_BYTES,
        "sequence":sequence,"commit":commit.to_string(),"full_source_verified":true,"original_native_bound":true,
        "fixture_scope":"Legacy sorted bulk load; streamed stack8-byte keys/values, no input Vec retained",
        "setup_elapsed_ns":started.elapsed().as_nanos(),"memory":memory()?}),
    )?;
    command("attempt")?;
    let limits = HashLimits {
        max_entries: c.keys,
        max_index_bytes: c.index_budget,
        max_value_bytes: source::VALUE_BYTES,
        max_total_value_bytes: c.keys * source::VALUE_BYTES,
    };
    let started = Instant::now();
    let outcome = match read.prepare_lookup(b"kv", Options::Hash(limits)) {
        Err(ulmdb::Error::LimitExceeded) => json!({"kind":"limit_exceeded"}),
        Err(error) => json!({"kind":"unexpected_error","error":error.to_string()}),
        Ok(index) => {
            json!({"kind":"allocated","index_bytes":index.stats().index_bytes,"entries":index.stats().entries})
        }
    };
    let elapsed = started.elapsed().as_nanos();
    source::verify(&read, c.keys)?;
    let still_original = original_native
        .get(&first_key)?
        .ok_or("original native first key disappeared")?;
    if still_original != original_value || still_original.as_ptr() != original_value.as_ptr() {
        return Err("original prepared native reader changed after allocation failure".into());
    }
    let unchanged = read.sequence() == sequence && read.commit_identity() == commit;
    if !unchanged {
        return Err("snapshot identity changed after allocation attempt".into());
    }
    send(
        json!({"type":"attempt","outcome":outcome,"elapsed_ns":elapsed,
        "full_source_verified":true,"native_and_ordered_usable":true,"original_native_still_usable":true,"snapshot_unchanged":unchanged,
        "sequence":read.sequence(),"commit":read.commit_identity().to_string(),"memory":memory()?}),
    )?;
    // Parent restores this same child's original address-space limit. Retrying
    // identical admission options proves the first error was not a hash budget
    // rejection or a permanently poisoned/mutated read transaction.
    command("retry")?;
    let started = Instant::now();
    let retry = match read.prepare_lookup(b"kv", Options::Hash(limits)) {
        Ok(index) => {
            let stats = index.stats();
            if stats.entries != c.keys || stats.index_bytes > c.index_budget {
                return Err("successful retry has wrong cardinality/budget".into());
            }
            for id in 0..c.keys as u64 {
                if index.get(&id.to_be_bytes())? != Some(source::value(id).as_slice()) {
                    return Err("successful hash retry bytes mismatch".into());
                }
            }
            json!({"kind":"allocated","index_bytes":stats.index_bytes,"entries":stats.entries,"all_hash_values_verified":true})
        }
        Err(error) => json!({"kind":"unexpected_error","error":error.to_string()}),
    };
    source::verify(&read, c.keys)?;
    if read.sequence() != sequence || read.commit_identity() != commit {
        return Err("snapshot identity changed after retry".into());
    }
    let still_original = original_native
        .get(&first_key)?
        .ok_or("original native key disappeared after retry")?;
    if still_original != original_value || still_original.as_ptr() != original_value.as_ptr() {
        return Err("original prepared native reader changed after retry".into());
    }
    send(
        json!({"type":"retry","outcome":retry,"elapsed_ns":started.elapsed().as_nanos(),
        "full_source_verified":true,"native_and_ordered_usable":true,"original_native_still_usable":true,"snapshot_unchanged":true,
        "sequence":sequence,"commit":commit.to_string(),"memory":memory()?}),
    )?;
    command("finish")?;
    Ok(())
}
fn main() {
    if let Err(error) = config().and_then(|c| run(&c)) {
        eprintln!("allocation probe failed: {error}");
        std::process::exit(1);
    }
}
