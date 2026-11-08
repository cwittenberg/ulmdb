use std::path::Path;
use ulmdb_reference::workloads::{Config, PrefixProfile, Workload};

fn execute() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(5..=6).contains(&args.len()) {
        return Err(
            "usage: workload NEW_DIRECTORY PREFIXES KV QUERIES SEED [gapped|with-defaults]".into(),
        );
    }
    let number = |index: usize| args[index].parse::<usize>().map_err(|e| e.to_string());
    let config = Config {
        seed: args[4].parse::<u64>().map_err(|e| e.to_string())?,
        prefixes: number(1)?,
        kv: number(2)?,
        queries: number(3)?,
        prefix_profile: match args.get(5).map(String::as_str).unwrap_or("gapped") {
            "gapped" => PrefixProfile::Gapped,
            "with-defaults" => PrefixProfile::WithDefaults,
            _ => return Err("prefix profile must be gapped or with-defaults".into()),
        },
    };
    let w = Workload::generate(config)?;
    print!("{}", w.write_new(Path::new(&args[0]))?);
    Ok(())
}
fn main() {
    if let Err(e) = execute() {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}
