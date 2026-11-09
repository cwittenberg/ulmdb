//! CLI driver for the same resumable jobs used by application integrations.
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use ulmdb::{
    Error, Result,
    backup::{BackupControl, BackupJob, BackupLimits, CancellationToken, JobStep, WorkBudget},
};

pub(super) const OPTIONS: &str = "[--max-units N] [--max-bytes N] [--deadline-ms N] [--slice-units N] [--slice-bytes N] [--cancel-file PATH] [--progress]";

pub(super) struct Options {
    pub control: BackupControl,
    slice: WorkBudget,
    progress: bool,
    stop: Arc<AtomicBool>,
    watcher: Option<JoinHandle<()>>,
}
impl Drop for Options {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(watcher) = self.watcher.take() {
            watcher.thread().unpark();
            let _ = watcher.join();
        }
    }
}
fn number(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| Error::InvalidInput("backup option requires an unsigned integer"))
}
impl Options {
    pub fn parse(args: &[String]) -> Result<Self> {
        let mut limits = BackupLimits::UNLIMITED;
        let mut slice = WorkBudget {
            max_units: 4096,
            max_bytes: u64::MAX,
        };
        let mut progress = false;
        let mut cancel_file = None;
        let mut seen = BTreeSet::new();
        let mut args = args.iter();
        while let Some(option) = args.next() {
            if !seen.insert(option.as_str()) {
                return Err(Error::InvalidInput("duplicate backup option"));
            }
            if option == "--progress" {
                progress = true;
                continue;
            }
            let value = args.next().ok_or(Error::InvalidInput(OPTIONS))?;
            match option.as_str() {
                "--max-units" => limits.max_units = number(value)?,
                "--max-bytes" => limits.max_bytes = number(value)?,
                "--slice-units" => slice.max_units = number(value)?,
                "--slice-bytes" => slice.max_bytes = number(value)?,
                "--deadline-ms" => {
                    limits.deadline = Some(
                        Instant::now()
                            .checked_add(Duration::from_millis(number(value)?))
                            .ok_or(Error::InvalidInput(
                                "backup deadline exceeds platform clock",
                            ))?,
                    );
                }
                "--cancel-file" => cancel_file = Some(PathBuf::from(value)),
                _ => return Err(Error::InvalidInput(OPTIONS)),
            }
        }
        if slice.max_units == 0 || slice.max_bytes == 0 {
            return Err(Error::InvalidInput("backup slice limits must be nonzero"));
        }
        let token = CancellationToken::new();
        let control = BackupControl::new(limits).with_cancel_token(token.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let watcher = if let Some(path) = cancel_file {
            // Treat an inaccessible cancellation signal as a stop request.
            if path.try_exists().unwrap_or(true) {
                token.cancel();
            }
            let stop = stop.clone();
            Some(
                thread::Builder::new()
                    .name("ipdon-backup-cancel".into())
                    .spawn(move || {
                        while !stop.load(Ordering::Acquire) && !token.is_cancelled() {
                            if path.try_exists().unwrap_or(true) {
                                token.cancel();
                                break;
                            }
                            thread::park_timeout(Duration::from_millis(10));
                        }
                    })?,
            )
        } else {
            None
        };
        Ok(Self {
            control,
            slice,
            progress,
            stop,
            watcher,
        })
    }

    pub fn run<T: Send + 'static>(&self, mut job: BackupJob<T>) -> Result<T> {
        loop {
            match job.advance(self.slice)? {
                JobStep::Complete(value) => return Ok(value),
                JobStep::Pending { progress, required } => {
                    if self.progress {
                        eprintln!(
                            "backup work_units={} bytes={}",
                            progress.units, progress.bytes
                        );
                    }
                    if required.max_units > self.slice.max_units
                        || required.max_bytes > self.slice.max_bytes
                    {
                        return Err(Error::InvalidInput(
                            "backup slice is smaller than one native work item; increase --slice-units or --slice-bytes",
                        ));
                    }
                    thread::yield_now();
                }
            }
        }
    }
}
