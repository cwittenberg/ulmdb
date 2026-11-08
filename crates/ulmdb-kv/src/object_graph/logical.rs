//! One page's checked logical lengths. No ordinary key/value body is borrowed.
use crate::page::{HEADER, Page, u16_at};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result};
#[derive(Debug)]
pub enum SmallValue {
    Inline(Vec<u8>),
    Payload(Reference),
}
#[derive(Debug)]
pub struct LogicalRow {
    pub key_bytes: usize,
    pub value: SmallValue,
}
#[derive(Debug)]
pub struct LogicalPage {
    pub level: u16,
    pub children: Vec<Reference>,
    pub entries: u64,
    pub key_bytes: u64,
    pub value_bytes: u64,
    pub minimum_key: Option<usize>,
    pub maximum_key: usize,
    pub maximum_value: usize,
    pub small_values: Vec<LogicalRow>,
}
/// Inspect a page structurally, without comparing keys or reading overflow
/// bodies. `small_values` allows only bounded duplicate-bucket metadata; payload
/// references are returned for the caller to load in separate short scopes.
/// This summary is not a whole-graph/order certificate.
pub fn inspect_logical_page<S: ReadStore>(
    store: &S,
    at: Reference,
    small_values: Option<usize>,
) -> Result<LogicalPage> {
    if at.is_null() || !at.offset.is_multiple_of(store.page_size() as u64) {
        return Err(Error::Corrupt("logical page identity"));
    }
    let page = Page::load(store, at)?;
    let mut out = LogicalPage {
        level: page.level,
        children: Vec::new(),
        entries: 0,
        key_bytes: 0,
        value_bytes: 0,
        minimum_key: None,
        maximum_key: 0,
        maximum_value: 0,
        small_values: Vec::new(),
    };
    let mut occupied = vec![false; store.page_size()];
    let mut lowest = store.page_size();
    for i in 0..page.count {
        let (key, value, location) = if let Some(layout) = page.compact() {
            let cell = layout.cell(page.bytes, page.level == 0, page.count, i)?;
            (
                cell.key.len(),
                cell.value.len(),
                Some(SmallValue::Inline(if small_values.is_some() {
                    cell.value.to_vec()
                } else {
                    Vec::new()
                })),
            )
        } else {
            let raw = page.raw_cell(i)?;
            let start = usize::from(u16_at(page.bytes, HEADER + i * 2)?);
            let end = start.checked_add(raw.len()).ok_or(Error::LimitExceeded)?;
            let span = occupied
                .get_mut(start..end)
                .ok_or(Error::Corrupt("logical page cell extent"))?;
            if span.iter().any(|v| *v) {
                return Err(Error::Corrupt("logical page cell overlap"));
            }
            span.fill(true);
            lowest = lowest.min(start);
            let flags = u16_at(raw, 2)?;
            let header = if page.level == 0 { 8 } else { 20 };
            let key = if flags & 1 != 0 {
                store.payload_length(Reference::decode(&raw[header..header + 16])?)?
            } else {
                usize::from(u16_at(raw, 0)?)
            };
            let key_rep = if flags & 1 != 0 { 16 } else { key };
            let offset = header + key_rep;
            if page.level == 0 {
                let value = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
                let location = if flags & 2 != 0 {
                    let reference = Reference::decode(&raw[offset..offset + 16])?;
                    if store.payload_length(reference)? != value {
                        return Err(Error::Corrupt("logical value length mismatch"));
                    }
                    Some(SmallValue::Payload(reference))
                } else {
                    Some(SmallValue::Inline(if small_values.is_some() {
                        raw[offset..offset + value].to_vec()
                    } else {
                        Vec::new()
                    }))
                };
                (key, value, location)
            } else {
                (key, 0, None)
            }
        };
        out.minimum_key = Some(out.minimum_key.map_or(key, |old| old.min(key)));
        out.maximum_key = out.maximum_key.max(key);
        if page.level == 0 {
            out.entries = out.entries.checked_add(1).ok_or(Error::LimitExceeded)?;
            out.key_bytes = out
                .key_bytes
                .checked_add(key as u64)
                .ok_or(Error::LimitExceeded)?;
            out.value_bytes = out
                .value_bytes
                .checked_add(value as u64)
                .ok_or(Error::LimitExceeded)?;
            out.maximum_value = out.maximum_value.max(value);
            if let Some(limit) = small_values {
                if value > limit {
                    return Err(Error::Corrupt("logical small metadata limit"));
                }
                out.small_values.push(LogicalRow {
                    key_bytes: key,
                    value: location.unwrap(),
                });
            }
        }
    }
    if let Some(layout) = page.compact() {
        let end = HEADER + page.count * layout.stride(page.level == 0);
        if page.bytes[end..].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt("logical compact tail"));
        }
    } else {
        let raw = u16_at(page.bytes, 10)?;
        let upper = if store.page_size() == 65536 && raw == 0 {
            65536
        } else {
            usize::from(raw)
        };
        if lowest != upper {
            return Err(Error::Corrupt("logical page upper boundary"));
        }
    }
    if page.level != 0 {
        for i in 0..=page.count {
            out.children.push(page.child(store, i)?);
        }
    }
    Ok(out)
}
