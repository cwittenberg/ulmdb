//! Test-only worker startup fence for a transient inherited flock descriptor.
//! The failed checkpoint and prior trace are retained under comparisons/
//! reclaim-fast-path/diagnostics and indexed-inventory/diagnostics respectively.
use std::{
    path::Path,
    time::{Duration, Instant},
};
use ulmdb::{Environment, Error};

/// A parallel Command fork can inherit another thread's CLOEXEC flock until
/// exec, briefly outliving that initializer's handle. Causation of the observed
/// untraced Busy failures is inferred from the separately recorded trace.
/// Only crash-worker startup uses this helper; production and parent opens
/// remain nonblocking, and all non-Busy failures are reported immediately.
pub fn open_worker(path: impl AsRef<Path>) -> Environment {
    let path = path.as_ref();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(1);
    let mut attempts = 0;
    loop {
        attempts += 1;
        match Environment::open(path) {
            Ok(environment) => return environment,
            Err(Error::Busy) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!(
                "subprocess worker could not open {} after {attempts} attempts in {:?}: {error:?}",
                path.display(),
                started.elapsed(),
            ),
        }
    }
}
