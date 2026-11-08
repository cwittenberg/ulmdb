use crate::{
    data::{self, Config, Record},
    observe::Report,
    protocol::{Engine, Result},
};
use serde_json::json;
use std::{collections::BTreeMap, time::Instant};

pub fn phase(
    engine: &impl Engine,
    c: &Config,
    rows: &[Record],
    name: &str,
    revision: &mut u64,
    report: &mut Report,
) -> Result<BTreeMap<usize, [u8; 32]>> {
    let mut latest = BTreeMap::new();
    let mut state = c.seed
        ^ match name {
            "unpinned_before" => 0x8765_4321,
            "pinned" => 0x1234_5678,
            _ => 0x6521_8743,
        };
    for batch in [1, 64, 1000] {
        for sample in 0..c.samples {
            let groups: Vec<_> = (0..c.transactions)
                .map(|_| {
                    (0..batch)
                        .map(|_| {
                            let id = data::random(&mut state) as usize % rows.len();
                            *revision += 1;
                            (id, data::value(id as u64, *revision))
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let start = Instant::now();
            for (transaction, group) in groups.iter().enumerate() {
                if let Err(error) = engine.apply(rows, group) {
                    let diagnostics = engine.diagnostics();
                    return Err(format!("write phase={name} batch={batch} sample={sample} transaction={transaction}: {error}; diagnostics={diagnostics}").into());
                }
            }
            let elapsed = start.elapsed().as_nanos();
            for group in groups {
                for (id, value) in group {
                    latest.insert(id, value);
                }
            }
            report.samples.push(json!({"phase":"write","pin_state":name,"workload":format!("durable_batch{batch}"),"sample":sample,"operations":batch*c.transactions,"transactions":c.transactions,"elapsed_ns":elapsed,"ns_per_op":elapsed as f64/(batch*c.transactions) as f64,"ns_per_transaction":elapsed as f64/c.transactions as f64,"durability":"normal synchronized durable commit; no relaxed flags","timing_scope":"begin writer, ordinary put updates and durable commit; precomputed fixtures excluded"}));
        }
        report.rss(&format!("{name}_batch{batch}"));
    }
    Ok(latest)
}
pub fn apply_model(rows: &mut [Record], changes: BTreeMap<usize, [u8; 32]>) {
    for (id, value) in changes {
        rows[id].value = value;
    }
}
