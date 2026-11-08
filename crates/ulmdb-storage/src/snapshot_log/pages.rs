//! External-only small-redo page prototype. No File, allocator, decoder or unsafe.
//!
//! Paired certificates authenticate selected prefixes. A nonzero torn/bad
//! certificate fails closed, even if its predecessor still validates. Thus an
//! interrupted append can be unavailable; this does not promise power-cut
//! availability or distinguish that image from acknowledged media corruption.
//! Recovery here authenticates one packet locally. An OPEN packet terminates
//! the stream regardless of stale suffix bytes. A CLOSED result obliges the
//! outer dispatcher to validate its child at page_end against context() before
//! exposing the recovered stream; missing/unselected children are corruption.
use crate::{Error, Result};
use sha2::{Digest, Sha256};

pub const PAGE_BYTES: usize = 4096;
pub const MAX_BODY: usize = 1024;
pub const HEADER_BYTES: usize = 64;
pub const CERT_BYTES: usize = 80;
pub const FOOTER_BYTES: usize = 2 * CERT_BYTES;
pub const MAX_PACKET_BYTES: usize = 8192;
pub const MAX_COUNT: u16 = 2047;
pub const MAGIC: &[u8; 8] = b"ULMSPG04";
const HEADER_DOMAIN: &[u8; 8] = b"ULMSPH04";
const TX_DOMAIN: &[u8; 8] = b"ULMSPT04";
const CERT_DOMAIN: &[u8; 8] = b"ULMSPC04";
const CERT_MAGIC: &[u8; 4] = b"PGC4";
const COMPRESSED: u16 = 1 << 15;
const LENGTH_MASK: u16 = (1 << 11) - 1;
const CLOSED: u16 = 1 << 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Context {
    pub source: [u8; 16],
    pub generation: u64,
    pub sequence: u64,
    pub hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageState {
    base: Context,
    start: usize,
    end: usize,
    header: [u8; HEADER_BYTES],
    footer: [u8; FOOTER_BYTES],
    prefix_end: usize,
    count: u16,
    latest_slot: usize,
    closed: bool,
    sequence: u64,
    hash: [u8; 32],
}

/// A private immutable publication candidate. Its exact body remains borrowed
/// until the caller's synchronous write completes. Dropping it changes no state.
pub struct Prepared<'a> {
    next: PageState,
    prefix: [u8; HEADER_BYTES + 2],
    prefix_len: usize,
    body: &'a [u8],
    offset: usize,
    gap_len: usize,
}
impl<'a> Prepared<'a> {
    pub fn prefix(&self) -> &[u8] {
        &self.prefix[..self.prefix_len]
    }
    pub fn body(&self) -> &'a [u8] {
        self.body
    }
    pub fn gap_len(&self) -> usize {
        self.gap_len
    }
    pub fn footer(&self) -> &[u8; FOOTER_BYTES] {
        &self.next.footer
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn page_end(&self) -> usize {
        self.next.end
    }
    pub fn marker_len(&self) -> usize {
        0
    }
    pub fn next_state(&self) -> &PageState {
        &self.next
    }
    pub fn is_new_packet(&self) -> bool {
        self.prefix_len == HEADER_BYTES + 2
    }
    pub fn cert_slot(&self) -> usize {
        self.next.latest_slot
    }
    pub fn into_state(self) -> PageState {
        self.next
    }
}
/// Private close bytes only, deliberately without an install/into_state API.
/// Publishing them alone is forbidden: the caller must co-fence a fully
/// prepared child and publish that child's OPEN state after durability succeeds.
pub struct ClosingFooter {
    footer: [u8; FOOTER_BYTES],
    slot: usize,
    offset: usize,
}
impl ClosingFooter {
    pub fn footer(&self) -> &[u8; FOOTER_BYTES] {
        &self.footer
    }
    pub fn closing_slot(&self) -> usize {
        self.slot
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
}
fn header_hash(base: Context, start: usize, header: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(HEADER_DOMAIN);
    digest.update(base.source);
    digest.update((start as u64).to_le_bytes());
    digest.update(base.hash);
    digest.update(header);
    digest.finalize().into()
}
fn certificate_hash(base: Context, start: usize, header: &[u8], certificate: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(CERT_DOMAIN);
    digest.update(base.source);
    digest.update((start as u64).to_le_bytes());
    digest.update(base.hash);
    digest.update(header);
    digest.update(certificate);
    digest.finalize().into()
}
fn transaction_hash(
    source: [u8; 16],
    sequence: u64,
    parent: [u8; 32],
    tag: [u8; 2],
    body: &[u8],
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(TX_DOMAIN);
    digest.update(source);
    digest.update(sequence.to_le_bytes());
    digest.update(parent);
    digest.update(tag);
    digest.update(body);
    digest.finalize().into()
}
fn tag(body: &[u8], compressed: bool) -> Result<[u8; 2]> {
    if body.len() > MAX_BODY {
        return Err(Error::LimitExceeded);
    }
    Ok(((body.len() as u16) | if compressed { COMPRESSED } else { 0 }).to_le_bytes())
}
fn tag_length(tag: [u8; 2]) -> Result<(usize, bool)> {
    let tag = u16::from_le_bytes(tag);
    let length = usize::from(tag & LENGTH_MASK);
    if tag & !(LENGTH_MASK | COMPRESSED) != 0 || length > MAX_BODY {
        return Err(Error::Corrupt("packed page record tag"));
    }
    Ok((length, tag & COMPRESSED != 0))
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("checked fixed field"))
}
fn certificate(state: &PageState) -> [u8; CERT_BYTES] {
    let mut bytes = [0; CERT_BYTES];
    bytes[..4].copy_from_slice(CERT_MAGIC);
    bytes[4..6].copy_from_slice(&(state.prefix_end as u16).to_le_bytes());
    bytes[6..8]
        .copy_from_slice(&(state.count | if state.closed { CLOSED } else { 0 }).to_le_bytes());
    bytes[8..16].copy_from_slice(&state.sequence.to_le_bytes());
    bytes[16..48].copy_from_slice(&state.hash);
    let hash = certificate_hash(state.base, state.start, &state.header, &bytes[..48]);
    bytes[48..].copy_from_slice(&hash);
    bytes
}
impl PageState {
    pub fn new_packet<'a>(
        base: Context,
        start: usize,
        bank_capacity: usize,
        body: &'a [u8],
        compressed: bool,
    ) -> Result<Prepared<'a>> {
        let tag = tag(body, compressed)?;
        if !bank_capacity.is_multiple_of(PAGE_BYTES) {
            return Err(Error::InvalidInput("packed page bank geometry"));
        }
        let required = start
            .checked_add(HEADER_BYTES + 2 + body.len() + FOOTER_BYTES)
            .ok_or(Error::LimitExceeded)?;
        let end = required
            .checked_add(PAGE_BYTES - 1)
            .map(|end| end / PAGE_BYTES * PAGE_BYTES)
            .filter(|end| *end <= bank_capacity && *end - start <= MAX_PACKET_BYTES)
            .ok_or(Error::LimitExceeded)?;
        let sequence = base.sequence.checked_add(1).ok_or(Error::LimitExceeded)?;
        let mut header = [0; HEADER_BYTES];
        header[..8].copy_from_slice(MAGIC);
        header[8..16].copy_from_slice(&base.generation.to_le_bytes());
        header[16..24].copy_from_slice(&(end as u64).to_le_bytes());
        header[24..32].copy_from_slice(&base.sequence.to_le_bytes());
        let hash = header_hash(base, start, &header[..32]);
        header[32..].copy_from_slice(&hash);
        let mut state = Self {
            base,
            start,
            end,
            header,
            footer: [0; FOOTER_BYTES],
            prefix_end: HEADER_BYTES + 2 + body.len(),
            count: 1,
            latest_slot: 0,
            closed: false,
            sequence,
            hash: transaction_hash(base.source, sequence, base.hash, tag, body),
        };
        let cert = certificate(&state);
        state.footer[..CERT_BYTES].copy_from_slice(&cert);
        let mut prefix = [0; HEADER_BYTES + 2];
        prefix[..HEADER_BYTES].copy_from_slice(&header);
        prefix[HEADER_BYTES..].copy_from_slice(&tag);
        Ok(Prepared {
            offset: start,
            gap_len: end - start - FOOTER_BYTES - state.prefix_end,
            next: state,
            prefix,
            prefix_len: HEADER_BYTES + 2,
            body,
        })
    }
    pub fn prepare<'a>(&self, body: &'a [u8], compressed: bool) -> Result<Prepared<'a>> {
        if self.closed {
            return Err(Error::InvalidInput("packed page is closed"));
        }
        let tag = tag(body, compressed)?;
        let prefix_end = self
            .prefix_end
            .checked_add(2 + body.len())
            .filter(|end| *end <= self.end - self.start - FOOTER_BYTES)
            .ok_or(Error::LimitExceeded)?;
        let count = self
            .count
            .checked_add(1)
            .filter(|count| *count <= MAX_COUNT)
            .ok_or(Error::LimitExceeded)?;
        let sequence = self.sequence.checked_add(1).ok_or(Error::LimitExceeded)?;
        let mut next = self.clone();
        next.prefix_end = prefix_end;
        next.count = count;
        next.sequence = sequence;
        next.hash = transaction_hash(self.base.source, sequence, self.hash, tag, body);
        next.latest_slot = 1 - self.latest_slot;
        let at = next.latest_slot * CERT_BYTES;
        let cert = certificate(&next);
        next.footer[at..at + CERT_BYTES].copy_from_slice(&cert);
        let mut prefix = [0; HEADER_BYTES + 2];
        prefix[..2].copy_from_slice(&tag);
        Ok(Prepared {
            offset: self.start + self.prefix_end,
            gap_len: self.end - self.start - FOOTER_BYTES - prefix_end,
            next,
            prefix,
            prefix_len: 2,
            body,
        })
    }
    pub fn start(&self) -> usize {
        self.start
    }
    pub fn page_end(&self) -> usize {
        self.end
    }
    pub fn end(&self) -> usize {
        self.end
    }
    pub fn prefix_end(&self) -> usize {
        self.prefix_end
    }
    pub fn count(&self) -> u16 {
        self.count
    }
    pub fn active_slot(&self) -> usize {
        self.latest_slot
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn prepare_successor(&self) -> Result<ClosingFooter> {
        if self.closed {
            return Err(Error::InvalidInput("packed page is closed"));
        }
        let mut next = self.clone();
        next.closed = true;
        next.latest_slot = 1 - self.latest_slot;
        let at = next.latest_slot * CERT_BYTES;
        let certificate = certificate(&next);
        next.footer[at..at + CERT_BYTES].copy_from_slice(&certificate);
        Ok(ClosingFooter {
            footer: next.footer,
            slot: next.latest_slot,
            offset: self.end - FOOTER_BYTES,
        })
    }
    /// None distinguishes a full prefix from space for a valid empty body.
    pub fn available_body(&self) -> Option<usize> {
        if self.closed || self.count >= MAX_COUNT {
            return None;
        }
        (self.end - self.start - FOOTER_BYTES - self.prefix_end)
            .checked_sub(2)
            .map(|bytes| bytes.min(MAX_BODY))
    }
    pub fn footer(&self) -> &[u8; FOOTER_BYTES] {
        &self.footer
    }
    pub fn context(&self) -> Context {
        Context {
            sequence: self.sequence,
            hash: self.hash,
            ..self.base
        }
    }
}
#[derive(Clone, Copy)]
struct Certificate {
    prefix_end: usize,
    count: u16,
    sequence: u64,
    hash: [u8; 32],
    slot: usize,
    closed: bool,
}
fn parse_certificate(
    bytes: &[u8],
    base: Context,
    start: usize,
    header: &[u8],
    footer_start: usize,
    slot: usize,
) -> Result<Option<Certificate>> {
    if bytes.iter().all(|&byte| byte == 0) {
        return Ok(None);
    }
    if bytes[48..] != certificate_hash(base, start, header, &bytes[..48])
        || bytes[..4] != *CERT_MAGIC
    {
        return Err(Error::Corrupt("packed page certificate checksum"));
    }
    let prefix_end = usize::from(u16::from_le_bytes(bytes[4..6].try_into().unwrap()));
    let count_flags = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
    if count_flags & !(LENGTH_MASK | CLOSED) != 0 {
        return Err(Error::Corrupt("packed page certificate count flags"));
    }
    let count = count_flags & LENGTH_MASK;
    let sequence = u64_at(bytes, 8);
    if count == 0
        || prefix_end > footer_start
        || prefix_end < HEADER_BYTES + 2 * usize::from(count)
        || base.sequence.checked_add(u64::from(count)) != Some(sequence)
    {
        return Err(Error::Corrupt("packed page certificate bounds"));
    }
    Ok(Some(Certificate {
        prefix_end,
        count,
        sequence,
        hash: bytes[16..48].try_into().unwrap(),
        slot,
        closed: count_flags & CLOSED != 0,
    }))
}

pub struct Recovered<'a> {
    state: PageState,
    records: Records<'a>,
}
impl<'a> Recovered<'a> {
    pub fn state(&self) -> &PageState {
        &self.state
    }
    pub fn records(&self) -> Records<'a> {
        self.records.clone()
    }
    pub fn into_state(self) -> PageState {
        self.state
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record<'a> {
    pub sequence: u64,
    pub compressed: bool,
    pub stored: &'a [u8],
}
#[derive(Clone)]
pub struct Records<'a> {
    bytes: &'a [u8],
    at: usize,
    remaining: u16,
    sequence: u64,
}
impl<'a> Iterator for Records<'a> {
    type Item = Record<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        // Every extent/tag and the final sequence was checked before this
        // borrowing iterator could be constructed. No partial result escapes.
        let tag = u16::from_le_bytes(self.bytes[self.at..self.at + 2].try_into().unwrap());
        let length = usize::from(tag & LENGTH_MASK);
        let start = self.at + 2;
        self.at = start + length;
        self.remaining -= 1;
        self.sequence += 1;
        Some(Record {
            sequence: self.sequence,
            compressed: tag & COMPRESSED != 0,
            stored: &self.bytes[start..self.at],
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            usize::from(self.remaining),
            Some(usize::from(self.remaining)),
        )
    }
}
impl ExactSizeIterator for Records<'_> {}
impl std::iter::FusedIterator for Records<'_> {}

/// Authenticate this packet's complete selected prefix. CLOSED results require
/// outer-dispatcher child validation before exposing the recovered stream.
pub fn recover(bank: &[u8], start: usize, base: Context) -> Result<Option<Recovered<'_>>> {
    if !bank.len().is_multiple_of(PAGE_BYTES) {
        return Err(Error::Corrupt("packed page bank geometry"));
    }
    let header_end = start
        .checked_add(HEADER_BYTES)
        .filter(|end| *end <= bank.len())
        .ok_or(Error::Corrupt("packed page truncated header"))?;
    let header: [u8; HEADER_BYTES] = bank[start..header_end].try_into().unwrap();
    if header[..8] != *MAGIC {
        return Err(Error::Corrupt("packed page magic"));
    }
    if u64_at(&header, 8) != base.generation {
        return Ok(None);
    }
    if u64_at(&header, 24) != base.sequence
        || header[32..] != header_hash(base, start, &header[..32])
    {
        return Err(Error::Corrupt("packed page header checksum/context"));
    }
    let end = usize::try_from(u64_at(&header, 16))
        .ok()
        .filter(|end| *end <= bank.len() && end.is_multiple_of(PAGE_BYTES))
        .filter(|end| {
            end.checked_sub(start).is_some_and(|length| {
                (HEADER_BYTES + 2 + FOOTER_BYTES..=MAX_PACKET_BYTES).contains(&length)
            })
        })
        .ok_or(Error::Corrupt("packed page extent"))?;
    let footer_start = end - start - FOOTER_BYTES;
    let footer: [u8; FOOTER_BYTES] = bank[end - FOOTER_BYTES..end].try_into().unwrap();
    // Self-check both certificates first. In particular a torn nonzero future
    // certificate never silently falls back to its still-valid predecessor.
    let first = parse_certificate(&footer[..CERT_BYTES], base, start, &header, footer_start, 0)?;
    let second = parse_certificate(&footer[CERT_BYTES..], base, start, &header, footer_start, 1)?;
    let (earlier, latest) = match (first, second) {
        (None, None) => return Ok(None),
        (Some(cert), None) | (None, Some(cert)) if cert.count == 1 && !cert.closed => (None, cert),
        (Some(a), Some(b)) => {
            if a.closed || b.closed {
                if a.closed == b.closed
                    || a.count != b.count
                    || a.sequence != b.sequence
                    || a.prefix_end != b.prefix_end
                    || a.hash != b.hash
                {
                    return Err(Error::Corrupt("packed page closing certificate pair"));
                }
                let (open, closed) = if a.closed { (b, a) } else { (a, b) };
                (Some(open), closed)
            } else {
                let (early, late) = if a.count < b.count { (a, b) } else { (b, a) };
                if early.count.checked_add(1) != Some(late.count)
                    || early.sequence.checked_add(1) != Some(late.sequence)
                    || early.prefix_end >= late.prefix_end
                {
                    return Err(Error::Corrupt("packed page certificate adjacency"));
                }
                (Some(early), late)
            }
        }
        _ => {
            return Err(Error::Corrupt(
                "packed page missing predecessor certificate",
            ));
        }
    };
    let packet = &bank[start..end];
    let mut at = HEADER_BYTES;
    let mut hash = base.hash;
    for count in 1..=latest.count {
        let tag_end = at
            .checked_add(2)
            .filter(|end| *end <= latest.prefix_end)
            .ok_or(Error::Corrupt("packed page record prefix"))?;
        let tag: [u8; 2] = packet[at..tag_end].try_into().unwrap();
        let (length, _) = tag_length(tag)?;
        if count == 1 {
            let canonical_end = start
                .checked_add(HEADER_BYTES + 2 + length + FOOTER_BYTES)
                .and_then(|required| required.checked_add(PAGE_BYTES - 1))
                .map(|required| required / PAGE_BYTES * PAGE_BYTES)
                .ok_or(Error::Corrupt("packed page canonical extent"))?;
            if canonical_end != end {
                return Err(Error::Corrupt("packed page canonical extent"));
            }
        }
        let record_end = tag_end
            .checked_add(length)
            .filter(|end| *end <= latest.prefix_end)
            .ok_or(Error::Corrupt("packed page record extent"))?;
        let sequence = base
            .sequence
            .checked_add(u64::from(count))
            .ok_or(Error::Corrupt("packed page record sequence"))?;
        hash = transaction_hash(
            base.source,
            sequence,
            hash,
            tag,
            &packet[tag_end..record_end],
        );
        at = record_end;
        for certificate in [earlier, Some(latest)].into_iter().flatten() {
            if certificate.count == count
                && (certificate.prefix_end != at
                    || certificate.hash != hash
                    || certificate.sequence != sequence)
            {
                return Err(Error::Corrupt("packed page record chain/prefix"));
            }
        }
    }
    let state = PageState {
        base,
        start,
        end,
        header,
        footer,
        prefix_end: latest.prefix_end,
        count: latest.count,
        latest_slot: latest.slot,
        closed: latest.closed,
        sequence: latest.sequence,
        hash: latest.hash,
    };
    Ok(Some(Recovered {
        state,
        records: Records {
            bytes: &packet[..latest.prefix_end],
            at: HEADER_BYTES,
            remaining: latest.count,
            sequence: base.sequence,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Context {
        Context {
            source: [0x71; 16],
            generation: 7,
            sequence: 129,
            hash: [0xa3; 32],
        }
    }
    fn commit(bank: &mut [u8], prepared: Prepared<'_>) -> PageState {
        let mut at = prepared.offset();
        for bytes in [prepared.prefix(), prepared.body()] {
            bank[at..at + bytes.len()].copy_from_slice(bytes);
            at += bytes.len();
        }
        bank[at..at + prepared.gap_len()].fill(0);
        at += prepared.gap_len();
        bank[at..at + FOOTER_BYTES].copy_from_slice(prepared.footer());
        at += FOOTER_BYTES;
        assert_eq!(at, prepared.page_end());
        bank[at..at + prepared.marker_len()].fill(0);
        prepared.into_state()
    }
    fn refresh_header(bank: &mut [u8], start: usize, context: Context) {
        let digest = header_hash(context, start, &bank[start..start + 32]);
        bank[start + 32..start + HEADER_BYTES].copy_from_slice(&digest);
    }
    fn refresh_cert(bank: &mut [u8], start: usize, end: usize, slot: usize, context: Context) {
        let at = end - FOOTER_BYTES + slot * CERT_BYTES;
        let digest = certificate_hash(
            context,
            start,
            &bank[start..start + HEADER_BYTES],
            &bank[at..at + 48],
        );
        bank[at + 48..at + CERT_BYTES].copy_from_slice(&digest);
    }
    fn two_records() -> (Vec<u8>, PageState) {
        let mut bank = vec![0; 3 * PAGE_BYTES];
        let first = commit(
            &mut bank,
            PageState::new_packet(base(), 280, 3 * PAGE_BYTES, b"old", false).unwrap(),
        );
        let next = first.prepare(b"new-value", true).unwrap();
        let state = commit(&mut bank, next);
        (bank, state)
    }
    #[test]
    fn binary_lengths_and_arbitrary_starts_round_trip_without_padding_before_packet() {
        for start in [0, 1, 280, 4095, 4096, 6001] {
            for length in [0, 1, 64, MAX_BODY] {
                for compressed in [false, true] {
                    let body: Vec<_> = (0..length)
                        .map(|i| (i.wrapping_mul(179) & 255) as u8)
                        .collect();
                    let mut bank = vec![0x61; 4 * PAGE_BYTES];
                    let prepared =
                        PageState::new_packet(base(), start, bank.len(), &body, compressed)
                            .unwrap();
                    assert_eq!(prepared.offset(), start);
                    assert_eq!(prepared.prefix().len(), HEADER_BYTES + 2);
                    assert_eq!(
                        prepared.prefix()[HEADER_BYTES..],
                        ((length as u16) | if compressed { COMPRESSED } else { 0 }).to_le_bytes()
                    );
                    assert!(prepared.page_end().is_multiple_of(PAGE_BYTES));
                    assert!(prepared.page_end() - start <= MAX_PACKET_BYTES);
                    assert!(prepared.gap_len() < MAX_PACKET_BYTES);
                    let state = commit(&mut bank, prepared);
                    assert!(bank[..start].iter().all(|&byte| byte == 0x61));
                    let recovered = recover(&bank, start, base()).unwrap().unwrap();
                    assert_eq!(recovered.state(), &state);
                    assert_eq!(
                        recovered.records().collect::<Vec<_>>(),
                        vec![Record {
                            sequence: 130,
                            compressed,
                            stored: &body
                        }]
                    );
                }
            }
        }
    }
    #[test]
    fn append_alternates_certificates_preserves_old_slot_and_private_abort() {
        let mut bank = vec![0; 2 * PAGE_BYTES];
        let first = commit(
            &mut bank,
            PageState::new_packet(base(), 280, 2 * PAGE_BYTES, b"old", false).unwrap(),
        );
        assert!(first.footer[CERT_BYTES..].iter().all(|&byte| byte == 0));
        let old_footer = first.footer;
        let before = bank.clone();
        let prepared = first.prepare(b"not-yet-committed", false).unwrap();
        assert_eq!(&prepared.footer()[..CERT_BYTES], &old_footer[..CERT_BYTES]);
        assert_eq!(prepared.prefix().len(), 2);
        assert_eq!(prepared.offset(), first.start + first.prefix_end);
        drop(prepared);
        assert_eq!(bank, before);
        assert_eq!(first.count(), 1);
        let second = commit(&mut bank, first.prepare(b"second", false).unwrap());
        assert_eq!(second.latest_slot, 1);
        let third_prepared = second.prepare(b"third", false).unwrap();
        assert_eq!(
            &third_prepared.footer()[CERT_BYTES..],
            &second.footer[CERT_BYTES..]
        );
        let third = commit(&mut bank, third_prepared);
        assert_eq!(third.latest_slot, 0);
        let recovered = recover(&bank, 280, base()).unwrap().unwrap();
        assert_eq!(recovered.state(), &third);
        assert_eq!(
            recovered.records().map(|r| r.stored).collect::<Vec<_>>(),
            vec![b"old".as_slice(), b"second".as_slice(), b"third".as_slice()]
        );
    }
    #[test]
    fn packets_fill_split_and_bind_the_next_page_to_latest_context() {
        let mut bank = vec![0; 8 * PAGE_BYTES];
        let body = [0x35; 1024];
        let mut state = commit(
            &mut bank,
            PageState::new_packet(base(), 4095, 8 * PAGE_BYTES, &body, false).unwrap(),
        );
        let first_end = state.page_end();
        while let Ok(prepared) = state.prepare(&body, false) {
            state = commit(&mut bank, prepared);
        }
        assert!(matches!(
            state.prepare(&body, false),
            Err(Error::LimitExceeded)
        ));
        let before = bank.clone();
        let next_context = state.context();
        let close = state.prepare_successor().unwrap();
        bank[close.offset()..close.offset() + FOOTER_BYTES].copy_from_slice(close.footer());
        let next = commit(
            &mut bank,
            PageState::new_packet(next_context, first_end, 8 * PAGE_BYTES, &[], false).unwrap(),
        );
        assert_eq!(
            &bank[..first_end - FOOTER_BYTES],
            &before[..first_end - FOOTER_BYTES]
        );
        assert_eq!(next.start(), first_end);
        assert_eq!(next.context().sequence, next_context.sequence + 1);
        let first = recover(&bank, 4095, base()).unwrap().unwrap();
        assert!(first.state().is_closed());
        assert_eq!(first.state().context(), state.context());
        assert_eq!(
            recover(&bank, first_end, next_context)
                .unwrap()
                .unwrap()
                .state(),
            &next
        );
        assert!(recover(&bank, first_end, base()).is_err());
    }
    #[test]
    fn header_body_certificates_and_context_corruption_fail_closed() {
        let (bank, state) = two_records();
        let start = state.start();
        // Generation mismatch is explicitly unselected; every other header
        // byte, each certificate byte, and selected body bytes fail closed.
        for at in (0..8).chain(16..HEADER_BYTES) {
            let mut bad = bank.clone();
            bad[start + at] ^= 1;
            assert!(recover(&bad, start, base()).is_err(), "header byte {at}");
        }
        let mut stale = bank.clone();
        stale[start + 8] ^= 1;
        assert!(recover(&stale, start, base()).unwrap().is_none());
        for at in state.page_end() - FOOTER_BYTES..state.page_end() {
            let mut bad = bank.clone();
            bad[at] ^= 1;
            assert!(
                recover(&bad, start, base()).is_err(),
                "certificate byte {at}"
            );
        }
        for at in start + HEADER_BYTES..start + state.prefix_end() {
            let mut bad = bank.clone();
            bad[at] ^= 1;
            assert!(recover(&bad, start, base()).is_err(), "record byte {at}");
        }
        let mut wrong = base();
        wrong.source[0] ^= 1;
        assert!(recover(&bank, start, wrong).is_err());
        let mut wrong = base();
        wrong.hash[0] ^= 1;
        assert!(recover(&bank, start, wrong).is_err());
        let mut wrong = base();
        wrong.sequence += 1;
        assert!(recover(&bank, start, wrong).is_err());
        let mut wrong = base();
        wrong.generation += 1;
        assert!(recover(&bank, start, wrong).unwrap().is_none());
    }
    #[test]
    fn both_zero_is_frontier_but_nonzero_torn_future_certificate_is_corrupt() {
        let mut bank = vec![0; 2 * PAGE_BYTES];
        let first = commit(
            &mut bank,
            PageState::new_packet(base(), 280, 2 * PAGE_BYTES, b"old", false).unwrap(),
        );
        let footer_start = first.page_end() - FOOTER_BYTES;
        bank[footer_start..first.page_end()].fill(0);
        assert!(recover(&bank, first.start(), base()).unwrap().is_none());
        bank[footer_start..footer_start + CERT_BYTES].copy_from_slice(&first.footer[..CERT_BYTES]);
        bank[footer_start + CERT_BYTES] = 1;
        assert!(recover(&bank, first.start(), base()).is_err());
    }
    #[test]
    fn old_certificate_selects_unchanged_prefix_despite_unselected_future_body() {
        let mut bank = vec![0; 2 * PAGE_BYTES];
        let first = commit(
            &mut bank,
            PageState::new_packet(base(), 280, 2 * PAGE_BYTES, b"old", false).unwrap(),
        );
        let prepared = first.prepare(b"uncommitted future bytes", true).unwrap();
        let at = prepared.offset();
        bank[at..at + 2].copy_from_slice(prepared.prefix());
        bank[at + 2..at + 2 + prepared.body().len()].copy_from_slice(prepared.body());
        // The old footer is deliberately untouched by this interrupted write.
        let recovered = recover(&bank, first.start(), base()).unwrap().unwrap();
        assert_eq!(recovered.state(), &first);
        assert_eq!(
            recovered.records().map(|r| r.stored).collect::<Vec<_>>(),
            vec![b"old".as_slice()]
        );
        let replacement = recovered
            .into_state()
            .prepare(b"replacement", false)
            .unwrap();
        let next = commit(&mut bank, replacement);
        assert_eq!(
            recover(&bank, first.start(), base())
                .unwrap()
                .unwrap()
                .state(),
            &next
        );
    }
    #[test]
    fn valid_self_checks_cannot_hide_bad_counts_prefixes_tags_or_chains() {
        let (original, state) = two_records();
        let (start, end) = (state.start(), state.page_end());
        let latest_at = end - CERT_BYTES;
        for field in [4, 6, 8, 16] {
            let mut bad = original.clone();
            bad[latest_at + field] ^= 4;
            refresh_cert(&mut bad, start, end, 1, base());
            assert!(
                recover(&bad, start, base()).is_err(),
                "self-checked field {field}"
            );
        }
        // A valid latest certificate must not hide corruption in the older
        // certified prefix: recovery promises to authenticate BOTH witnesses.
        for field in [4, 16] {
            let mut bad = original.clone();
            bad[end - FOOTER_BYTES + field] ^= 1;
            refresh_cert(&mut bad, start, end, 0, base());
            assert!(recover(&bad, start, base()).is_err());
        }
        let mut missing = original.clone();
        missing[end - FOOTER_BYTES..end - CERT_BYTES].fill(0);
        assert!(recover(&missing, start, base()).is_err());
        for invalid_tag in [0x0800u16, 0x1000, 0x4000, 1025, 2047] {
            let mut bad = original.clone();
            bad[start + HEADER_BYTES..start + HEADER_BYTES + 2]
                .copy_from_slice(&invalid_tag.to_le_bytes());
            assert!(recover(&bad, start, base()).is_err());
        }
        let mut bad_end = original.clone();
        bad_end[start + 16..start + 24].copy_from_slice(&u64::MAX.to_le_bytes());
        refresh_header(&mut bad_end, start, base());
        assert!(recover(&bad_end, start, base()).is_err());
        let mut no_alignment = original.clone();
        no_alignment[start + 16..start + 24].copy_from_slice(&((end - 1) as u64).to_le_bytes());
        refresh_header(&mut no_alignment, start, base());
        assert!(recover(&no_alignment, start, base()).is_err());
        let mut oversized = original.clone();
        let new_end = end + PAGE_BYTES;
        let old_footer: [u8; FOOTER_BYTES] = original[end - FOOTER_BYTES..end].try_into().unwrap();
        oversized[start + 16..start + 24].copy_from_slice(&(new_end as u64).to_le_bytes());
        refresh_header(&mut oversized, start, base());
        oversized[new_end - FOOTER_BYTES..new_end].copy_from_slice(&old_footer);
        for slot in [0, 1] {
            refresh_cert(&mut oversized, start, new_end, slot, base());
        }
        assert!(matches!(
            recover(&oversized, start, base()),
            Err(Error::Corrupt("packed page canonical extent"))
        ));
    }
    #[test]
    fn capacity_sequence_limits_and_iterator_lifetimes_are_bounded() {
        assert!(matches!(
            PageState::new_packet(base(), 0, PAGE_BYTES - 1, b"", false),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            PageState::new_packet(base(), PAGE_BYTES - 1, PAGE_BYTES, &[0; 1024], false),
            Err(Error::LimitExceeded)
        ));
        assert!(matches!(
            PageState::new_packet(base(), usize::MAX, PAGE_BYTES, b"", false),
            Err(Error::LimitExceeded)
        ));
        assert!(matches!(
            PageState::new_packet(base(), 0, PAGE_BYTES, &[0; 1025], false),
            Err(Error::LimitExceeded)
        ));
        let mut exhausted = base();
        exhausted.sequence = u64::MAX;
        assert!(matches!(
            PageState::new_packet(exhausted, 0, PAGE_BYTES, b"", false),
            Err(Error::LimitExceeded)
        ));
        let mut bank = vec![0; PAGE_BYTES];
        let first = commit(
            &mut bank,
            PageState::new_packet(base(), 0, PAGE_BYTES, b"", false).unwrap(),
        );
        assert_eq!(first.page_end(), PAGE_BYTES);
        let recovered = recover(&bank, 0, base()).unwrap().unwrap();
        let mut records = recovered.records();
        drop(recovered); // Iterator borrows immutable bank, not the wrapper.
        assert_eq!(records.len(), 1);
        assert_eq!(records.next().unwrap().stored, b"");
        assert_eq!(records.size_hint(), (0, Some(0)));
        assert_eq!(records.next(), None);
        assert_eq!(records.next(), None);
        assert!(recover(&bank[..PAGE_BYTES - 1], 0, base()).is_err());
        assert!(recover(&bank, PAGE_BYTES - 1, base()).is_err());
    }
    #[test]
    fn recovery_preserves_actual_certificate_slot_instead_of_count_parity() {
        let (mut bank, state) = two_records();
        let at = state.end - FOOTER_BYTES;
        let first: [u8; CERT_BYTES] = bank[at..at + CERT_BYTES].try_into().unwrap();
        let second: [u8; CERT_BYTES] = bank[at + CERT_BYTES..state.end].try_into().unwrap();
        bank[at..at + CERT_BYTES].copy_from_slice(&second);
        bank[at + CERT_BYTES..state.end].copy_from_slice(&first);
        // Slot position is deliberately not part of certificate hashing.
        let recovered = recover(&bank, state.start, base()).unwrap().unwrap();
        assert_eq!(recovered.state().active_slot(), 0);
        assert_eq!(recovered.state().context(), state.context());
        let recovered = recovered.into_state();
        let prepared = recovered.prepare(b"third", false).unwrap();
        assert!(!prepared.is_new_packet());
        assert_eq!(prepared.cert_slot(), 1);
        assert_eq!(&prepared.footer()[..CERT_BYTES], &second);
        let third = commit(&mut bank, prepared);
        assert_eq!(
            recover(&bank, state.start, base())
                .unwrap()
                .unwrap()
                .state(),
            &third
        );
    }
    #[test]
    fn empty_record_space_is_distinct_from_full_and_sequence_overflow_is_prepared_only() {
        let mut context = base();
        context.sequence = u64::MAX - 1;
        let prepared = PageState::new_packet(context, 0, PAGE_BYTES, &[], false).unwrap();
        assert_eq!(prepared.next_state().context().sequence, u64::MAX);
        assert!(matches!(
            prepared.next_state().prepare(&[], false),
            Err(Error::LimitExceeded)
        ));
        let mut bank = vec![0; PAGE_BYTES];
        let mut state = commit(
            &mut bank,
            PageState::new_packet(base(), 0, PAGE_BYTES, &[], false).unwrap(),
        );
        let bytes = [0u8; MAX_BODY];
        while let Some(available) = state.available_body() {
            state = commit(
                &mut bank,
                state.prepare(&bytes[..available], false).unwrap(),
            );
        }
        assert_eq!(state.prefix_end, state.end - state.start - FOOTER_BYTES);
        assert!(matches!(
            state.prepare(&[], false),
            Err(Error::LimitExceeded)
        ));
        let recovered = recover(&bank, 0, base()).unwrap().unwrap();
        assert_eq!(recovered.state(), &state);
        assert_eq!(recovered.records().count(), usize::from(state.count));
    }
    fn locally_verified_page_stream(
        bank: &[u8],
        mut start: usize,
        mut context: Context,
    ) -> Result<Vec<PageState>> {
        // Test-only stand-in for the outer dispatcher. It must never translate
        // an unselected mandatory child into a normal stream termination.
        let mut states = Vec::new();
        loop {
            let page =
                recover(bank, start, context)?.ok_or(Error::Corrupt("required packed child"))?;
            let closed = page.state().is_closed();
            context = page.state().context();
            start = page.state().end();
            states.push(page.into_state());
            if !closed {
                return Ok(states);
            }
        }
    }
    #[test]
    fn open_terminal_never_writes_marker_and_close_requires_a_verified_successor() {
        let mut bank = vec![0xa5; 4 * PAGE_BYTES];
        let first = commit(
            &mut bank,
            PageState::new_packet(base(), 280, 4 * PAGE_BYTES, b"first", false).unwrap(),
        );
        assert!(!first.is_closed());
        assert_eq!(bank[first.end()], 0xa5);
        let before = bank.clone();
        let old_active = first.active_slot();
        let close = first.prepare_successor().unwrap();
        assert_eq!(close.closing_slot(), 1 - old_active);
        let old_at = old_active * CERT_BYTES;
        assert_eq!(
            &close.footer()[old_at..old_at + CERT_BYTES],
            &first.footer()[old_at..old_at + CERT_BYTES]
        );
        assert_eq!(bank, before);
        drop(close); // No close-only state/publication capability is exposed.
        assert!(!first.is_closed());

        let child = commit(
            &mut bank,
            PageState::new_packet(first.context(), first.end(), 4 * PAGE_BYTES, b"child", true)
                .unwrap(),
        );
        // An unreferenced complete future page is ignored by the OPEN parent.
        assert_eq!(
            locally_verified_page_stream(&bank, first.start(), base())
                .unwrap()
                .len(),
            1
        );
        let close = first.prepare_successor().unwrap();
        bank[close.offset()..close.offset() + FOOTER_BYTES].copy_from_slice(close.footer());
        let closed = recover(&bank, first.start(), base())
            .unwrap()
            .unwrap()
            .into_state();
        assert!(closed.is_closed());
        assert_eq!(closed.active_slot(), close.closing_slot());
        assert_eq!(closed.context(), first.context());
        assert_eq!(closed.available_body(), None);
        assert!(matches!(
            closed.prepare(b"forbidden", false),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            closed.prepare_successor(),
            Err(Error::InvalidInput(_))
        ));
        let stream = locally_verified_page_stream(&bank, first.start(), base()).unwrap();
        assert_eq!(stream.len(), 2);
        assert_eq!(stream[1], child);
        let mut stale = bank.clone();
        stale[child.start() + 8] ^= 1;
        assert!(locally_verified_page_stream(&stale, first.start(), base()).is_err());
        let mut missing = bank.clone();
        missing[child.start()..child.end()].fill(0);
        assert!(locally_verified_page_stream(&missing, first.start(), base()).is_err());
    }
    #[test]
    fn malformed_self_checked_closing_pairs_and_reserved_count_bits_are_rejected() {
        let (original, state) = two_records();
        let (start, end) = (state.start(), state.end());
        let close = state.prepare_successor().unwrap();
        let mut closed = original.clone();
        closed[close.offset()..end].copy_from_slice(close.footer());
        assert!(
            recover(&closed, start, base())
                .unwrap()
                .unwrap()
                .state()
                .is_closed()
        );
        let open_slot = state.active_slot();
        let closed_slot = close.closing_slot();
        let field_at = |slot| end - FOOTER_BYTES + slot * CERT_BYTES;
        let cases = [
            (open_slot, state.count | CLOSED),         // Both closed.
            (closed_slot, state.count),                // Both open with equal count.
            (closed_slot, (state.count - 1) | CLOSED), // Closed adjacent is invalid.
            (closed_slot, state.count | 0x0800),
            (closed_slot, state.count | 0x4000 | CLOSED),
            (closed_slot, CLOSED), // Zero count.
        ];
        for (slot, flags) in cases {
            let mut bad = closed.clone();
            let at = field_at(slot);
            bad[at + 6..at + 8].copy_from_slice(&flags.to_le_bytes());
            refresh_cert(&mut bad, start, end, slot, base());
            assert!(recover(&bad, start, base()).is_err(), "flags {flags:04x}");
        }
        for field in [4, 8, 16] {
            let mut bad = closed.clone();
            bad[field_at(closed_slot) + field] ^= 1;
            refresh_cert(&mut bad, start, end, closed_slot, base());
            assert!(recover(&bad, start, base()).is_err());
        }
        let mut lone_closed = closed;
        lone_closed[field_at(open_slot)..field_at(open_slot) + CERT_BYTES].fill(0);
        assert!(recover(&lone_closed, start, base()).is_err());
        let mut closed_one = original;
        closed_one[end - CERT_BYTES..end].fill(0);
        closed_one[end - FOOTER_BYTES + 6..end - FOOTER_BYTES + 8]
            .copy_from_slice(&(CLOSED | 1).to_le_bytes());
        refresh_cert(&mut closed_one, start, end, 0, base());
        assert!(recover(&closed_one, start, base()).is_err());
    }
    #[test]
    fn count_2048_is_refused_even_when_an_empty_record_still_physically_fits() {
        let mut bank = vec![0; 3 * PAGE_BYTES];
        let start = PAGE_BYTES - (HEADER_BYTES + 2 + MAX_BODY + FOOTER_BYTES) + 1;
        let body = [0; MAX_BODY];
        let mut state = commit(
            &mut bank,
            PageState::new_packet(base(), start, 3 * PAGE_BYTES, &body, false).unwrap(),
        );
        while state.count < MAX_COUNT {
            state = commit(&mut bank, state.prepare(&[], false).unwrap());
        }
        assert!(state.end - state.start - FOOTER_BYTES - state.prefix_end >= 2);
        assert_eq!(state.available_body(), None);
        assert!(matches!(
            state.prepare(&[], false),
            Err(Error::LimitExceeded)
        ));
        let mut forged = bank.clone();
        let next_prefix = state.prefix_end + 2;
        forged[start + state.prefix_end..start + next_prefix].copy_from_slice(&[0; 2]);
        let slot = 1 - state.active_slot();
        let at = state.end - FOOTER_BYTES + slot * CERT_BYTES;
        forged[at..at + CERT_BYTES].copy_from_slice(
            &state.footer[state.active_slot() * CERT_BYTES..(state.active_slot() + 1) * CERT_BYTES],
        );
        forged[at + 4..at + 6].copy_from_slice(&(next_prefix as u16).to_le_bytes());
        forged[at + 6..at + 8].copy_from_slice(&2048u16.to_le_bytes());
        forged[at + 8..at + 16].copy_from_slice(&(state.sequence + 1).to_le_bytes());
        let hash = transaction_hash(base().source, state.sequence + 1, state.hash, [0; 2], &[]);
        forged[at + 16..at + 48].copy_from_slice(&hash);
        refresh_cert(&mut forged, start, state.end, slot, base());
        assert!(matches!(
            recover(&forged, start, base()),
            Err(Error::Corrupt("packed page certificate count flags"))
        ));
        assert_eq!(
            recover(&bank, start, base()).unwrap().unwrap().state(),
            &state
        );
    }
    #[test]
    fn actual_inline_layout_is_recorded_without_heap_or_size_guesses() {
        println!(
            "PageState={} OptionPageState={} Prepared={} ClosingFooter={}",
            std::mem::size_of::<PageState>(),
            std::mem::size_of::<Option<PageState>>(),
            std::mem::size_of::<Prepared<'_>>(),
            std::mem::size_of::<ClosingFooter>()
        );
        assert!(std::mem::size_of::<PageState>() <= 512);
        assert!(std::mem::size_of::<Prepared<'_>>() <= 640);
    }
}
