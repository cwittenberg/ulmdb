#![cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, sync::Arc};
use ulmdb::{DatabaseKind, Environment, PublicationMode, ReadAhead, ResidencyPolicy};
use ulmdb_storage::transform::{Checksum, ChecksumConfig, TransformContext};
struct Sum;
impl Checksum for Sum {
    fn compute(&self, src: &[u8], dst: &mut [u8], _: Option<&[u8]>) -> ulmdb::Result<()> {
        dst.copy_from_slice(&Sha256::digest(src));
        Ok(())
    }
}
struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn locked_at(pointer: usize) -> bool {
    let text = fs::read_to_string("/proc/self/smaps").unwrap();
    let mut selected = false;
    for line in text.lines() {
        let first = line.split_whitespace().next().unwrap_or("");
        if let Some((a, b)) = first.split_once('-') {
            if let (Ok(a), Ok(b)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16)) {
                selected = (a..b).contains(&pointer);
            }
        }
        if selected && line.starts_with("VmFlags:") {
            return line.split_whitespace().any(|f| f == "lo");
        }
    }
    false
}
#[test]
fn required_fixed_constructor_locks_actual_values_with_exact_budget_and_demand_peer_retention() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        for checksum in [false, true] {
            let d = Dir(std::env::temp_dir().join(format!(
                "fixed-required-{}",
                ulmdb_storage::new_identity().unwrap()
            )));
            let context = checksum.then(|| {
                Arc::new(
                    TransformContext::new(
                        Some(ChecksumConfig {
                            callback: Arc::new(Sum),
                            bytes: 32,
                        }),
                        None,
                        8 << 20,
                    )
                    .unwrap(),
                )
            });
            let mut e = Environment::create_with_transform_options(
                &d.0,
                mode,
                ResidencyPolicy::Demand,
                ReadAhead::Normal,
                context.clone(),
            )
            .unwrap();
            e.enable_fixed_map(1 << 20, None).unwrap();
            let mut w = e.begin_write().unwrap();
            w.create_database(b"data", DatabaseKind::Kv).unwrap();
            w.put(b"data", b"k", &vec![7; 12_000]).unwrap();
            w.commit().unwrap();
            let demand = e.begin_read();
            let value = demand.get(b"data", b"k").unwrap().unwrap();
            let pointer = value.as_ptr() as usize;
            let page = ulmdb_storage::ResidencyController::new(ResidencyPolicy::RequiredLocked {
                max_bytes: 0,
            })
            .unwrap()
            .live_status()
            .page_size
            .unwrap();
            let budget = demand
                .file_manifest()
                .unwrap()
                .segments
                .iter()
                .map(|entry| entry.length.div_ceil(page) * page)
                .sum::<u64>();
            drop(e);
            let required = match Environment::open_fixed_map_with_transform_options(
                &d.0,
                ResidencyPolicy::RequiredLocked { max_bytes: budget },
                ReadAhead::Normal,
                context.clone(),
            ) {
                Ok(e) => e,
                Err(ulmdb::Error::Io(error))
                    if matches!(error.raw_os_error(), Some(libc::EPERM) | Some(libc::ENOMEM)) =>
                {
                    eprintln!("FIXED_REQUIRED_OS_DECLINED {error}");
                    return;
                }
                Err(error) => panic!("{error}"),
            };
            let report = required.residency_report();
            assert_eq!(report.locked_bytes, budget);
            assert_eq!(required.residency_status().reserved_bytes, budget);
            assert_eq!(
                required
                    .begin_read()
                    .get(b"data", b"k")
                    .unwrap()
                    .unwrap()
                    .as_ptr() as usize,
                pointer
            );
            assert!(locked_at(pointer));
            drop(required);
            assert!(
                locked_at(pointer),
                "demand peer still owns the charged physical slot"
            );
            assert_eq!(value, vec![7; 12_000]);
            assert!(matches!(
                Environment::open_fixed_map_with_transform_options(
                    &d.0,
                    ResidencyPolicy::RequiredLocked {
                        max_bytes: budget - 1
                    },
                    ReadAhead::Normal,
                    context.clone()
                ),
                Err(ulmdb::Error::LimitExceeded)
            ));
            assert_eq!(demand.get(b"data", b"k").unwrap().unwrap(), vec![7; 12_000]);
            drop(demand);
            assert!(!locked_at(pointer));
            let reopened = Environment::open_fixed_map_with_transform_options(
                &d.0,
                ResidencyPolicy::Demand,
                ReadAhead::Normal,
                context.clone(),
            )
            .unwrap();
            assert_eq!(
                reopened
                    .begin_read()
                    .get(b"data", b"k")
                    .unwrap()
                    .unwrap()
                    .as_ptr() as usize,
                pointer
            );
            drop(reopened);
            if let Some(ctx) = context {
                assert_eq!(ctx.decoded_bytes(), 0);
            }
        }
    }
}
