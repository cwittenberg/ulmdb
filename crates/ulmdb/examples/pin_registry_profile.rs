//! Real registry generations, no synthesized headers or directory members.
//! Run a release build with NEW_DIRECTORY REPEAT [MAX_HISTORY=2048].
use std::{fs, hint::black_box, path::PathBuf, time::Instant};
use ulmdb_storage::{
    Manifest, WriterFence, checkpoint_bytes,
    coordination::{Coordination, PinDiscovery, PinDiscoveryBudget},
    create_layout, publish,
};

fn drain(
    discovery: &mut PinDiscovery,
    owner: &mut ulmdb_storage::coordination::MaintenanceLease,
) -> ulmdb_storage::Result<usize> {
    let mut work = 0;
    loop {
        let progress = discovery.advance(owner, PinDiscoveryBudget { records: 16 })?;
        work += progress.work_units;
        drop(progress.pins);
        if progress.complete {
            return Ok(work);
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if !(3..=4).contains(&args.len()) {
        return Err("usage: pin_registry_profile NEW_DIRECTORY REPEAT [MAX_HISTORY=2048]".into());
    }
    let path = PathBuf::from(&args[1]);
    let repeat: u64 = args[2].parse()?;
    let max_history: usize = args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(2048);
    create_layout(&path)?;
    let manifest = Manifest::empty()?;
    publish(&path, &manifest)?;
    let checkpoint = checkpoint_bytes(&manifest)?;
    let coordination = Coordination::open(&path, manifest.uuid, Default::default())?;
    let writer = WriterFence::try_lock(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.join("LOCK"))?,
    )?;
    let mut owner = coordination.try_maintenance()?;
    let mut discovery = owner.begin_pin_discovery(&writer)?;
    drain(&mut discovery, &mut owner)?;
    drop(owner);
    println!("metric,repeat,history,operations,elapsed_ns,generation_directories,work_units");
    for history in 0..=max_history {
        if history != 0 {
            // Seal while a real owner pin is live, then release before discovery.
            let pin = coordination.try_capture()?.pin_checkpoint(&checkpoint)?;
            let mut owner = coordination.try_maintenance()?;
            assert!(discovery.refresh(&mut owner)?);
            drop(owner);
            drop(pin);
            let mut owner = coordination.try_maintenance()?;
            drain(&mut discovery, &mut owner)?;
        }
        if ![0, 16, 256, 2048, max_history].contains(&history) {
            continue;
        }
        let directories = fs::read_dir(path.join(".ulmdb-coordination/pins"))?
            .filter_map(|v| v.ok())
            .filter(|v| v.file_type().is_ok_and(|t| t.is_dir()))
            .count();
        let operations = 32;
        let start = Instant::now();
        for _ in 0..operations {
            let pin = coordination.try_capture()?.pin_checkpoint(&checkpoint)?;
            black_box(pin.record_path());
            drop(pin);
        }
        println!(
            "pin_admission,{repeat},{history},{operations},{},{directories},0",
            start.elapsed().as_nanos()
        );
        let start = Instant::now();
        for _ in 0..operations {
            let mut owner = coordination.try_maintenance()?;
            let scan = owner.scan()?;
            assert!(scan.live_roots().is_empty());
            scan.cleanup_stale()?;
        }
        println!(
            "full_scan,{repeat},{history},{operations},{},{directories},0",
            start.elapsed().as_nanos()
        );
        let start = Instant::now();
        let mut owner = coordination.try_maintenance()?;
        let mut fresh = owner.begin_pin_discovery(&writer)?;
        let work = drain(&mut fresh, &mut owner)?;
        println!(
            "fresh_discovery,{repeat},{history},1,{},{directories},{work}",
            start.elapsed().as_nanos()
        );
    }
    Ok(())
}
