//! One publication engine for owned and borrowed RL2 canonical encodings.
use super::*;
use wal_codec_v2::Transition;
#[allow(clippy::too_many_arguments)]
pub(super) fn publish(
    file: &crate::transform::ResidentFile,
    boot: Boot,
    capacity: u64,
    capacity_probe: CapacityProbe,
    slot: &mut usize,
    cached: &mut [[u8; BLOCK]; 2],
    poisoned: &mut bool,
    transition: Transition<'_>,
    receipt: WalReceipt,
    commit: u128,
    bytes: &[u8],
) -> Result<WalReceipt> {
    let start = receipt.offset;
    if !file.can_write_extent(start, bytes.len() as u64)? {
        return Err(Error::LimitExceeded);
    }
    for (expected, at) in cached.iter().zip(HEADS) {
        let mut actual = [0; BLOCK];
        file.read_exact_at(&mut actual, at)?;
        if actual != *expected {
            *poisoned = true;
            return Err(Error::Corrupt("resident active witness changed"));
        }
    }
    if capacity_probe.length(file)? != capacity {
        *poisoned = true;
        return Err(Error::Corrupt("resident active capacity changed"));
    }
    let inactive = 1 - *slot;
    let witness = wire::head(boot, capacity, Witness { receipt, commit });
    let end = receipt.end()?;
    let appended = (|| {
        point("resident2.commit.before_append")?;
        write_cut(file, bytes, start, "resident2.commit.half_record")?;
        point("resident2.commit.after_append")?;
        point("resident2.commit.before_data_sync")?;
        file.sync_data()?;
        point("resident2.commit.after_data_sync")?;
        Ok::<(), Error>(())
    })();
    if let Err(error) = appended {
        let reset = (|| {
            point("resident2.commit.before_reset")?;
            tail::clear(file, start, end)?;
            point("resident2.commit.after_reset")?;
            file.sync_data()?;
            point("resident2.commit.after_reset_sync")?;
            Ok::<(), Error>(())
        })();
        if reset.is_err() {
            *poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        return Err(error);
    }
    let selected = (|| {
        point("resident2.commit.before_witness")?;
        write_cut(
            file,
            &witness,
            HEADS[inactive],
            "resident2.commit.half_witness",
        )?;
        point("resident2.commit.after_witness")?;
        point("resident2.commit.before_witness_sync")?;
        file.sync_data()?;
        point("resident2.commit.after_witness_sync")?;
        point("resident2.commit.before_ack")?;
        Ok::<(), Error>(())
    })();
    if selected.is_err() {
        *poisoned = true;
        return Err(Error::OutcomeUnknown);
    }
    cached[inactive] = witness;
    *slot = inactive;
    transition.activate();
    Ok(receipt)
}
