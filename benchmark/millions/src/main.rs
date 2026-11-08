mod lmdb_backend;
mod report;
mod sqlite;
#[cfg(not(feature = "resident"))]
mod ulmdb_backend;
#[cfg(feature = "resident")]
#[path = "ulmdb_resident.rs"]
mod ulmdb_backend;
mod workload;

use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn sha_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn main() {
    if let Err(error) = run() {
        eprintln!("benchmark failed: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args = workload::Args::parse()?;
    let workload = workload::Workload::new(&args.config);
    let result = match args.engine.as_str() {
        "ulmdb" => ulmdb_backend::run(&args.dir, &args.config, &workload)?,
        "lmdb" => lmdb_backend::run(&args.dir, &args.config, &workload)?,
        "sqlite" => sqlite::run(&args.dir, &args.config, &workload)?,
        _ => unreachable!(),
    };
    let storage_after_close_reopen = report::storage(&args.dir)?;
    let executable = std::env::current_exe()?;
    let selection = if cfg!(feature = "resident") {
        "ULMDB restored Resident iteration34 feature profile; CompactV2 Borrowed; no Legacy fallback"
    } else {
        "ULMDB public default Legacy; explicit default-file benchmark profile"
    };
    let report = serde_json::json!({"schema_version":1,"status":"complete","engine":args.engine,"backend":result.backend,"versions":result.versions,"config":args.config,"workload":workload.identity,"rows":result.rows,"settings":result.settings,"validation":result.validation,"setup_ns":result.setup_ns,"close_reopen_ns":result.close_reopen_ns,"maintenance":result.maintenance,"engine_stats":result.engine_stats,"storage_before_close":result.storage_before_close,"storage_after_close_reopen":storage_after_close_reopen,"build":{"executable_sha256":sha_file(&executable)?,"rustc":include_str!(env!("BENCH_RUSTC_INFO")),"profile":"release ThinLTO codegen-units=1 when built with --release","target_arch":std::env::consts::ARCH,"target_os":std::env::consts::OS,"resident_profile":cfg!(feature="resident")},"scope":{"qualified":false,"single_process_single_thread":true,"warm_reads":true,"read_snapshot_binding_outside_timer":true,"operation_timing":"aggregate Instant includes loop, checksum/black_box, errors and transaction begin/commit; no per-operation clock","write_durability":"normal durable acknowledged commits; engine-specific fence/checkpoint mechanisms differ","cold":"close/reopen without dropping OS caches; full reopened value oracle is outside close_reopen_ns","backend_selection":selection}});
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
