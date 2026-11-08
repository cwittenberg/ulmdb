//! Private format04 stream recovery. No file writes or partial result escapes.
//!
//! An OPEN packet terminates the stream without a next-page marker. CLOSED
//! requires a fully validated immediate child; every unselected child outcome
//! is corruption. Returned payloads own their bytes and are published only
//! after this complete dispatcher succeeds.
use super::{
    COMPACT_TX_HEADER, COMPACT_TX_MAGIC, COMPRESSED_TX_MAGIC, FOOTER, FrameFormat, MARKER, TX_END,
    TX_HEADER, TX_MAGIC, codec, complete_footer, copy_payload, frame_checksum, pages, read_u64,
    record_extent,
};
use crate::{Error, Result};

#[derive(Debug)]
pub(super) struct RecoveredBank {
    pub(super) tail: usize,
    pub(super) logical_tail: usize,
    pub(super) context: pages::Context,
    pub(super) page: Option<pages::PageState>,
    pub(super) dirty_tail: bool,
    pub(super) transactions: Vec<Vec<u8>>,
    pub(super) replay_bytes: u64,
    pub(super) frames: u64,
    pub(super) packed_pages: u64,
    pub(super) packed_records: u64,
    pub(super) closed_pages: u64,
}

fn frontier(mut recovered: RecoveredBank, dirty: bool, required: bool) -> Result<RecoveredBank> {
    if required {
        return Err(Error::Corrupt("packed stream missing required child"));
    }
    recovered.dirty_tail = dirty;
    Ok(recovered)
}

/// A damaged nonzero page magic cannot become an ordinary unknown frontier
/// merely because the compact-frame footer recognizer cannot see page certs.
/// This is bounded recognition, not authority to accept a packet: actual page
/// validation still requires its header/context, both certificates and chains.
fn recognizable_page(bank: &[u8], start: usize, context: pages::Context) -> bool {
    if bank.len().saturating_sub(start) < pages::HEADER_BYTES
        || read_u64(bank, start + 8) != context.generation
    {
        return false;
    }
    let Ok(end) = usize::try_from(read_u64(bank, start + 16)) else {
        return false;
    };
    if end > bank.len()
        || !end.is_multiple_of(pages::PAGE_BYTES)
        || !end.checked_sub(start).is_some_and(|length| {
            (pages::HEADER_BYTES + 2 + pages::FOOTER_BYTES..=pages::MAX_PACKET_BYTES)
                .contains(&length)
        })
    {
        return false;
    }
    let footer = end - pages::FOOTER_BYTES;
    bank[footer..footer + 4] == *b"PGC4"
        || bank[footer + pages::CERT_BYTES..footer + pages::CERT_BYTES + 4] == *b"PGC4"
}

fn unselected_frame(
    recovered: RecoveredBank,
    bank: &[u8],
    required: bool,
) -> Result<RecoveredBank> {
    if recognizable_page(bank, recovered.tail, recovered.context) {
        return Err(Error::Corrupt("packed stream damaged page magic"));
    }
    frontier(recovered, true, required)
}

fn logical_end(previous: usize, stored: &[u8], compressed: bool, capacity: usize) -> Result<usize> {
    let length = if compressed {
        codec::decoded_length(stored)?
    } else {
        stored.len()
    };
    previous
        .checked_add(record_extent(COMPACT_TX_HEADER, length)?)
        .filter(|end| *end <= capacity)
        .ok_or(Error::LimitExceeded)
}

pub(super) fn recover(
    bank: &[u8],
    initial_tail: usize,
    initial_logical_tail: usize,
    context: pages::Context,
    capacity: usize,
) -> Result<RecoveredBank> {
    if capacity != bank.len()
        || !capacity.is_multiple_of(pages::PAGE_BYTES)
        || initial_tail > capacity
        || initial_logical_tail > capacity
    {
        return Err(Error::Corrupt("packed stream initial bounds"));
    }
    let mut recovered = RecoveredBank {
        tail: initial_tail,
        logical_tail: initial_logical_tail,
        context,
        page: None,
        dirty_tail: false,
        transactions: Vec::new(),
        replay_bytes: 0,
        frames: 0,
        packed_pages: 0,
        packed_records: 0,
        closed_pages: 0,
    };
    let mut required = false;
    loop {
        let start = recovered.tail;
        let remaining = capacity - start;
        if remaining == 0 {
            return frontier(recovered, false, required);
        }
        recovered.replay_bytes += remaining.min(MARKER) as u64;
        if remaining < MARKER {
            return frontier(recovered, true, required);
        }
        let magic = &bank[start..start + MARKER];
        if magic == [0; MARKER] {
            // Preserve format03's existing zero-frontier policy. A required
            // child can never terminate here, even with a zeroed header.
            return frontier(recovered, false, required);
        }
        if magic == pages::MAGIC {
            let page = pages::recover(bank, start, recovered.context)?;
            let Some(page) = page else {
                recovered.replay_bytes += (pages::HEADER_BYTES - MARKER) as u64;
                if read_u64(bank, start + 8) == recovered.context.generation {
                    recovered.replay_bytes += pages::FOOTER_BYTES as u64;
                }
                return frontier(recovered, true, required);
            };
            let state = page.state();
            recovered.replay_bytes += (state.prefix_end() + pages::FOOTER_BYTES - MARKER) as u64;

            // Authenticate the entire packet above, then preflight ALL raw
            // logical extents before reserving/copying/decompressing any of
            // its payloads. A small packed encoding cannot expand the budget.
            let mut next_logical = recovered.logical_tail;
            for record in page.records() {
                next_logical =
                    logical_end(next_logical, record.stored, record.compressed, capacity)?;
            }
            recovered
                .transactions
                .try_reserve(usize::from(state.count()))
                .map_err(|_| Error::LimitExceeded)?;
            for record in page.records() {
                let payload = if record.compressed {
                    codec::decode(record.stored)?
                } else {
                    copy_payload(record.stored)?
                };
                recovered.transactions.push(payload);
            }
            recovered.logical_tail = next_logical;
            recovered.context = state.context();
            recovered.tail = state.page_end();
            recovered.frames += u64::from(state.count());
            recovered.packed_records += u64::from(state.count());
            recovered.packed_pages += 1;
            if state.is_closed() {
                recovered.closed_pages += 1;
                recovered.page = None;
                required = true;
                continue;
            }
            // OPEN is terminal. Do not examine or repair a marker at its end;
            // this would both weaken its authority and initialize another page.
            recovered.page = Some(page.into_state());
            return Ok(recovered);
        }

        let compressed = magic == COMPRESSED_TX_MAGIC;
        if magic != COMPACT_TX_MAGIC && !compressed {
            let complete = complete_footer(bank, start, COMPACT_TX_HEADER, 16, TX_END)
                || (magic == TX_MAGIC && complete_footer(bank, start, TX_HEADER, 40, TX_END));
            if complete {
                return Err(Error::Corrupt("packed stream complete foreign frame"));
            }
            return unselected_frame(recovered, bank, required);
        }
        if remaining < COMPACT_TX_HEADER + FOOTER {
            return unselected_frame(recovered, bank, required);
        }
        recovered.replay_bytes += (COMPACT_TX_HEADER - MARKER) as u64;
        if read_u64(bank, start + 8) != recovered.context.generation {
            return unselected_frame(recovered, bank, required);
        }
        let Ok(length) = usize::try_from(read_u64(bank, start + 16)) else {
            return unselected_frame(recovered, bank, required);
        };
        let Ok(extent) = record_extent(COMPACT_TX_HEADER, length) else {
            return unselected_frame(recovered, bank, required);
        };
        if extent > remaining {
            return unselected_frame(recovered, bank, required);
        }
        let footer = start + COMPACT_TX_HEADER + length;
        recovered.replay_bytes += (extent - COMPACT_TX_HEADER) as u64;
        if bank[footer + 32..footer + 40] != *TX_END {
            return unselected_frame(recovered, bank, required);
        }
        if read_u64(bank, footer + 40) != extent as u64
            || bank[footer + FOOTER..start + extent]
                .iter()
                .any(|&byte| byte != 0)
        {
            return Err(Error::Corrupt("packed stream complete frame footer"));
        }
        let sequence = recovered
            .context
            .sequence
            .checked_add(1)
            .ok_or(Error::Corrupt("packed stream frame sequence"))?;
        // Format04 keeps the exact format03 compact-frame codec for every
        // fallback, including tiny metadata/control transactions.
        let digest = frame_checksum(
            FrameFormat::Compact,
            recovered.context.source,
            sequence,
            recovered.context.hash,
            &bank[start..footer],
        );
        if bank[footer..footer + 32] != digest {
            return Err(Error::Corrupt("packed stream complete frame checksum"));
        }
        let stored = &bank[start + COMPACT_TX_HEADER..footer];
        let next_logical = logical_end(recovered.logical_tail, stored, compressed, capacity)?;
        recovered
            .transactions
            .try_reserve(1)
            .map_err(|_| Error::LimitExceeded)?;
        let payload = if compressed {
            codec::decode(stored)?
        } else {
            copy_payload(stored)?
        };
        recovered.transactions.push(payload);
        recovered.logical_tail = next_logical;
        recovered.context.sequence = sequence;
        recovered.context.hash = digest;
        recovered.tail = start + extent;
        recovered.frames += 1;
        recovered.page = None;
        required = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> pages::Context {
        pages::Context {
            source: [0x37; 16],
            generation: 7,
            sequence: 0,
            hash: [0x64; 32],
        }
    }
    fn packet(bank: &mut [u8], prepared: pages::Prepared<'_>) -> pages::PageState {
        let mut at = prepared.offset();
        for bytes in [prepared.prefix(), prepared.body()] {
            bank[at..at + bytes.len()].copy_from_slice(bytes);
            at += bytes.len();
        }
        bank[at..at + prepared.gap_len()].fill(0);
        at += prepared.gap_len();
        bank[at..at + pages::FOOTER_BYTES].copy_from_slice(prepared.footer());
        assert_eq!(at + pages::FOOTER_BYTES, prepared.page_end());
        prepared.into_state()
    }
    fn close(bank: &mut [u8], state: &pages::PageState) {
        let close = state.prepare_successor().unwrap();
        bank[close.offset()..close.offset() + pages::FOOTER_BYTES].copy_from_slice(close.footer());
    }
    fn framed(
        bank: &mut [u8],
        start: usize,
        mut ctx: pages::Context,
        stored: &[u8],
        compressed: bool,
    ) -> (usize, pages::Context) {
        let extent = record_extent(COMPACT_TX_HEADER, stored.len()).unwrap();
        let end = start + extent;
        bank[start..end].fill(0);
        bank[start..start + 8].copy_from_slice(if compressed {
            COMPRESSED_TX_MAGIC
        } else {
            COMPACT_TX_MAGIC
        });
        bank[start + 8..start + 16].copy_from_slice(&ctx.generation.to_le_bytes());
        bank[start + 16..start + 24].copy_from_slice(&(stored.len() as u64).to_le_bytes());
        bank[start + 24..start + 24 + stored.len()].copy_from_slice(stored);
        let footer = start + 24 + stored.len();
        let sequence = ctx.sequence + 1;
        let digest = frame_checksum(
            FrameFormat::Compact,
            ctx.source,
            sequence,
            ctx.hash,
            &bank[start..footer],
        );
        bank[footer..footer + 32].copy_from_slice(&digest);
        bank[footer + 32..footer + 40].copy_from_slice(TX_END);
        bank[footer + 40..footer + 48].copy_from_slice(&(extent as u64).to_le_bytes());
        if bank.len() - end >= MARKER {
            bank[end..end + MARKER].fill(0);
        }
        ctx.sequence = sequence;
        ctx.hash = digest;
        (end, ctx)
    }
    #[test]
    fn mixed_control_bulk_and_packets_recover_without_exposing_stale_suffix() {
        let capacity = pages::PAGE_BYTES * 8;
        let initial = 176;
        let mut bank = vec![0; capacity];
        let (at, ctx) = framed(&mut bank, initial, context(), b"control", false);
        let first = packet(
            &mut bank,
            pages::PageState::new_packet(ctx, at, capacity, b"first", false).unwrap(),
        );
        let second = packet(&mut bank, first.prepare(b"second", false).unwrap());
        close(&mut bank, &second);
        let bulk = vec![11; 2048];
        let (at, ctx) = framed(&mut bank, second.end(), second.context(), &bulk, false);
        let (at, ctx) = framed(&mut bank, at, ctx, b"later-control", false);
        let last = packet(
            &mut bank,
            pages::PageState::new_packet(ctx, at, capacity, b"last", false).unwrap(),
        );
        bank[last.end()..].fill(0xa9);
        let recovered = recover(&bank, initial, initial, context(), capacity).unwrap();
        assert_eq!(
            recovered.transactions,
            vec![
                b"control".to_vec(),
                b"first".to_vec(),
                b"second".to_vec(),
                bulk.clone(),
                b"later-control".to_vec(),
                b"last".to_vec()
            ]
        );
        let logical = [
            b"control".len(),
            5,
            6,
            bulk.len(),
            b"later-control".len(),
            4,
        ]
        .into_iter()
        .fold(initial, |sum, n| {
            sum + record_extent(COMPACT_TX_HEADER, n).unwrap()
        });
        assert_eq!(recovered.logical_tail, logical);
        assert_eq!(recovered.tail, last.end());
        assert_eq!(recovered.context, last.context());
        assert_eq!(recovered.page.unwrap(), last);
        assert!(!recovered.dirty_tail);
        assert_eq!(
            (
                recovered.frames,
                recovered.packed_pages,
                recovered.packed_records,
                recovered.closed_pages
            ),
            (6, 2, 3, 1)
        );
    }
    #[test]
    fn open_is_terminal_even_with_an_unreferenced_complete_child() {
        let capacity = 3 * pages::PAGE_BYTES;
        let mut bank = vec![0; capacity];
        let first = packet(
            &mut bank,
            pages::PageState::new_packet(context(), 176, capacity, b"selected", false).unwrap(),
        );
        framed(
            &mut bank,
            first.end(),
            first.context(),
            b"unselected",
            false,
        );
        let before = bank.clone();
        let recovered = recover(&bank, 176, 176, context(), capacity).unwrap();
        assert_eq!(recovered.transactions, vec![b"selected".to_vec()]);
        assert_eq!(recovered.tail, first.end());
        assert!(!recovered.dirty_tail);
        assert_eq!(
            recovered.replay_bytes,
            (first.prefix_end() + pages::FOOTER_BYTES) as u64
        );
        assert_eq!(bank, before);
    }
    #[test]
    fn closed_requires_current_valid_child_and_never_authorizes_repair() {
        let capacity = 3 * pages::PAGE_BYTES;
        let mut bank = vec![0; capacity];
        let first = packet(
            &mut bank,
            pages::PageState::new_packet(context(), 176, capacity, b"selected", false).unwrap(),
        );
        close(&mut bank, &first);
        let missing = bank.clone();
        assert!(matches!(
            recover(&bank, 176, 176, context(), capacity),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(bank, missing);
        let mut wrong = first.context();
        wrong.generation += 1;
        packet(
            &mut bank,
            pages::PageState::new_packet(wrong, first.end(), capacity, b"stale", false).unwrap(),
        );
        assert!(matches!(
            recover(&bank, 176, 176, context(), capacity),
            Err(Error::Corrupt(_))
        ));
        bank[first.end()..].fill(0);
        framed(&mut bank, first.end(), wrong, b"stale-control", false);
        assert!(matches!(
            recover(&bank, 176, 176, context(), capacity),
            Err(Error::Corrupt(_))
        ));
        bank[first.end()..].fill(0);
        let (end, _) = framed(&mut bank, first.end(), first.context(), b"valid", false);
        assert_eq!(
            recover(&bank, 176, 176, context(), capacity)
                .unwrap()
                .transactions
                .len(),
            2
        );
        bank[end - FOOTER] ^= 1;
        assert!(matches!(
            recover(&bank, 176, 176, context(), capacity),
            Err(Error::Corrupt(_))
        ));
    }
    #[test]
    fn bad_nonzero_packet_magic_with_complete_certificates_is_corrupt() {
        let capacity = 3 * pages::PAGE_BYTES;
        let mut bank = vec![0; capacity];
        packet(
            &mut bank,
            pages::PageState::new_packet(context(), 176, capacity, b"selected", false).unwrap(),
        );
        for magic in [*b"XLMSPG04", *COMPACT_TX_MAGIC, *TX_MAGIC] {
            let mut bad = bank.clone();
            bad[176..184].copy_from_slice(&magic);
            let before = bad.clone();
            assert!(matches!(
                recover(&bad, 176, 176, context(), capacity),
                Err(Error::Corrupt(_))
            ));
            assert_eq!(bad, before);
        }
    }
    #[test]
    fn ordinary_zero_stale_and_incomplete_frontiers_keep_existing_policy() {
        let capacity = 3 * pages::PAGE_BYTES;
        let mut bank = vec![0; capacity];
        assert!(
            !recover(&bank, 176, 176, context(), capacity)
                .unwrap()
                .dirty_tail
        );
        let mut wrong = context();
        wrong.generation += 1;
        packet(
            &mut bank,
            pages::PageState::new_packet(wrong, 176, capacity, b"old", false).unwrap(),
        );
        assert!(
            recover(&bank, 176, 176, context(), capacity)
                .unwrap()
                .dirty_tail
        );
        bank[176..].fill(0);
        framed(&mut bank, 176, wrong, b"old-control", false);
        assert!(
            recover(&bank, 176, 176, context(), capacity)
                .unwrap()
                .dirty_tail
        );
        bank[176..].fill(0);
        bank[176] = 0x73;
        let result = recover(&bank, 176, 176, context(), capacity).unwrap();
        assert!(result.dirty_tail);
        assert!(result.transactions.is_empty());
    }
    #[test]
    fn raw_logical_budget_precedes_packet_decode_and_compact_fallback_decode() {
        let capacity = 3 * pages::PAGE_BYTES;
        let mut scratch = [0; codec::SCRATCH_BYTES];
        let input = [7; 1024];
        let stored = codec::encode(&input, &mut scratch).unwrap();
        for packed in [false, true] {
            let mut bank = vec![0; capacity];
            if packed {
                packet(
                    &mut bank,
                    pages::PageState::new_packet(context(), 176, capacity, stored, true).unwrap(),
                );
            } else {
                framed(&mut bank, 176, context(), stored, true);
            }
            let cost = record_extent(COMPACT_TX_HEADER, input.len()).unwrap();
            assert!(matches!(
                recover(&bank, 176, capacity - cost + 1, context(), capacity),
                Err(Error::LimitExceeded)
            ));
            let recovered = recover(&bank, 176, 176, context(), capacity).unwrap();
            assert_eq!(recovered.transactions, vec![input.to_vec()]);
            assert_eq!(recovered.logical_tail, 176 + cost);
        }
    }
}
