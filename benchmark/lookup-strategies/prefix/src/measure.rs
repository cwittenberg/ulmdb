use crate::{
    arena::Arena,
    strategy::{Algorithm, fingerprint},
};
use std::{hint::black_box, time::Instant};
use ulmdb_core::{
    Prefix,
    storage::{Reference, Result},
};

#[derive(Clone, Copy)]
pub enum Operation {
    Exact,
    Lpm,
    Ancestors,
}
impl Operation {
    pub fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Lpm => "lpm",
            Self::Ancestors => "all_covering",
        }
    }
}
pub fn run(
    algo: &Algorithm,
    arena: &Arena,
    root: Reference,
    queries: &[Prefix],
    op: Operation,
    iterations: usize,
) -> Result<(f64, u64)> {
    let reader = algo.reader(arena, root)?;
    let mut checksum = 0u64;
    let start = Instant::now();
    for _ in 0..iterations {
        for p in queries {
            let q = black_box(*p);
            let item = match op {
                Operation::Exact => reader.exact(q)?,
                Operation::Lpm => reader.lpm(q)?,
                Operation::Ancestors => {
                    checksum = checksum.wrapping_add(algo.ancestors_checksum(arena, root, q)?);
                    continue;
                }
            };
            let value = item.map_or(0, |(p, v)| fingerprint(p, black_box(v)));
            checksum = checksum.wrapping_add(black_box(value));
        }
    }
    black_box(checksum);
    let ns = start.elapsed().as_nanos() as f64 / (queries.len() * iterations) as f64;
    Ok((ns, checksum))
}
