use super::*;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

struct Sources {
    directory: PathBuf,
    data: BTreeMap<u128, Vec<u8>>,
}
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
impl Sources {
    fn new(manifests: &[Manifest]) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "indexed-cursor-{:032x}",
            crate::new_identity().unwrap()
        ));
        fs::create_dir(&directory).unwrap();
        let mut data = BTreeMap::new();
        for m in manifests {
            let bytes = m.encode().unwrap();
            let p = directory.join(format!("{:032x}", m.generation));
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(p)
                .unwrap();
            f.write_all(&[0x9a; 37]).unwrap();
            f.write_all(&bytes).unwrap();
            f.write_all(&[0xac; 53]).unwrap();
            f.sync_all().unwrap();
            data.insert(m.generation, bytes);
        }
        Self { directory, data }
    }
    fn path(&self, origin: u128) -> PathBuf {
        self.directory.join(format!("{origin:032x}"))
    }
    fn file(&self, origin: u128) -> File {
        File::open(self.path(origin)).unwrap()
    }
    fn cursor(&self, m: &Manifest) -> IndexedDescriptorCursor {
        IndexedDescriptorCursor::new(
            self.file(m.generation),
            37,
            self.data[&m.generation].len(),
            crate::checkpoint_bytes(m).unwrap(),
            MAX_ENTRIES,
        )
        .unwrap()
    }
    fn supply(&self, c: &mut IndexedDescriptorCursor, origin: u128) {
        c.supply_origin(origin, self.file(origin), 37, self.data[&origin].len())
            .unwrap();
    }
    fn replace(&mut self, origin: u128, bytes: Vec<u8>) {
        let mut f = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(self.path(origin))
            .unwrap();
        f.write_all(&[0x9a; 37]).unwrap();
        f.write_all(&bytes).unwrap();
        f.write_all(&[0xac; 53]).unwrap();
        f.sync_all().unwrap();
        self.data.insert(origin, bytes);
    }
}
fn entry(id: u64) -> SegmentEntry {
    SegmentEntry {
        id,
        length: 4096 + id % 5,
        hash: [id as u8; 32],
        cohort: 0,
    }
}
fn history(count: usize) -> Vec<Manifest> {
    let mut a = Manifest::empty().unwrap();
    a.format_minor = 4;
    a.segments = (1..=count as u64).map(entry).collect();
    a.next_segment = count as u64 + 1;
    a.prepare_indexed(None).unwrap();
    let mut out = vec![a];
    for _ in 0..2 {
        let before = out.last().unwrap();
        let mut m = before.clone();
        m.generation = crate::new_identity().unwrap();
        m.previous = before.generation;
        m.parent = before.commit;
        m.commit = crate::new_identity().unwrap();
        m.sequence += 1;
        let id = m.next_segment;
        m.segments.push(entry(id));
        m.next_segment += 1;
        m.prepare_indexed(Some(before)).unwrap();
        out.push(m);
    }
    out
}
fn exhaust(
    c: &mut IndexedDescriptorCursor,
    sources: &Sources,
    budget: DescriptorCursorBudget,
) -> (usize, usize) {
    let (mut bytes, mut units) = (0, 0);
    for _ in 0..2_000_000 {
        let p = c.advance(budget).unwrap();
        assert!(p.read_bytes <= budget.bytes);
        assert!(p.work_units <= budget.entries);
        bytes += p.read_bytes;
        units += p.work_units;
        if let Some(origin) = p.needed_origin {
            sources.supply(c, origin);
        }
        if p.ready {
            return (bytes, units);
        }
    }
    panic!("bounded indexed cursor stalled");
}
struct MemorySource(BTreeMap<u128, Vec<u8>>);
impl crate::metadata::resolver::NodeSource for MemorySource {
    fn read(&mut self, origin: u128, offset: u64, length: u32, into: &mut [u8]) -> Result<()> {
        let b = self
            .0
            .get(&origin)
            .ok_or(Error::Corrupt("test origin absent"))?;
        let at = usize::try_from(offset).map_err(|_| Error::LimitExceeded)?;
        into.copy_from_slice(
            b.get(at..at + length as usize)
                .ok_or(Error::Corrupt("test node extent"))?,
        );
        Ok(())
    }
}
fn strict(bytes: &[u8], sources: &Sources) -> Result<Manifest> {
    let mut r =
        crate::metadata::Resolver::from_source(Box::new(MemorySource(sources.data.clone())));
    Manifest::decode_resolved(bytes, Some(&mut r))
}

#[test]
fn indexed_cursor_matches_full_decoder_across_real_shared_origins_and_exact_budgets() {
    for count in [1, 129] {
        let h = history(count);
        let m = h.last().unwrap();
        let sources = Sources::new(&h);
        let old = strict(&sources.data[&m.generation], &sources).unwrap();
        let mut c = sources.cursor(m);
        assert_eq!(
            c.advance(DescriptorCursorBudget {
                bytes: 0,
                entries: 0
            })
            .unwrap(),
            IndexedDescriptorProgress::default()
        );
        let budget = DescriptorCursorBudget {
            bytes: if count == 1 { 1 } else { 97 },
            entries: 1,
        };
        let (bytes, _) = exhaust(&mut c, &sources, budget);
        let out = c.finish().unwrap();
        assert_eq!(out.header().generation, old.generation);
        assert_eq!(out.header().segment_count, old.segments.len());
        assert_eq!(out.header().catalog, old.catalog);
        assert_eq!(out.header().next_database, old.next_database);
        assert_eq!(out.header().next_segment, old.next_segment);
        assert_eq!(out.header().uuid, old.uuid);
        assert_eq!(out.checkpoint(), &crate::checkpoint_bytes(m).unwrap());
        for (i, (a, b)) in out.entries().zip(old.segments.iter()).enumerate() {
            assert_eq!(
                (a.id, a.length, a.hash, a.cohort),
                (b.id, b.length, b.hash, b.cohort)
            );
            assert_eq!(out.entry(i).unwrap().id, a.id);
        }
        assert_eq!(out.entries().count(), old.segments.len());
        assert_eq!(
            out.metadata_origins().collect::<BTreeSet<_>>(),
            old.metadata_origins()
        );
        assert!(out.pages.values().all(|p| p.len() <= PAGE));
        let external = out.origins.len() - 1;
        assert_eq!(
            bytes,
            sources.data[&m.generation].len() + old.segments.len() * 256 - 160 + external * HEADER
        );
    }
    let mut empty = Manifest::empty().unwrap();
    empty.format_minor = 4;
    empty.prepare_indexed(None).unwrap();
    let sources = Sources::new(&[empty.clone()]);
    let mut c = sources.cursor(&empty);
    let (bytes, _) = exhaust(
        &mut c,
        &sources,
        DescriptorCursorBudget {
            bytes: 17,
            entries: 1,
        },
    );
    assert_eq!(bytes, HEADER);
    assert_eq!(c.finish().unwrap().entries().count(), 0);
}

#[test]
fn indexed_cursor_zero_read_or_work_units_resume_without_hidden_full_decode() {
    let h = history(3);
    let m = h.last().unwrap();
    let sources = Sources::new(&h);
    let mut c = sources.cursor(m);
    let p = c
        .advance(DescriptorCursorBudget {
            bytes: HEADER,
            entries: 0,
        })
        .unwrap();
    assert_eq!((p.read_bytes, p.work_units), (HEADER, 0));
    let p = c
        .advance(DescriptorCursorBudget {
            bytes: 0,
            entries: 1,
        })
        .unwrap();
    assert_eq!((p.read_bytes, p.work_units), (0, 1));
    let p = c
        .advance(DescriptorCursorBudget {
            bytes: 8,
            entries: 0,
        })
        .unwrap();
    assert_eq!((p.read_bytes, p.work_units), (8, 0));
    let p = c
        .advance(DescriptorCursorBudget {
            bytes: 0,
            entries: 1,
        })
        .unwrap();
    assert_eq!((p.read_bytes, p.work_units), (0, 1));
    exhaust(
        &mut c,
        &sources,
        DescriptorCursorBudget {
            bytes: 1,
            entries: 1,
        },
    );
    assert_eq!(c.finish().unwrap().entries().count(), 5);
}

fn reject(bytes: Vec<u8>, m: &Manifest, sources: &mut Sources, reauthenticate: bool) {
    let cp = if reauthenticate {
        crate::publication::record(m, Sha256::digest(&bytes).into())
    } else {
        crate::checkpoint_bytes(m).unwrap()
    };
    sources.replace(m.generation, bytes);
    let mut c = IndexedDescriptorCursor::new(
        sources.file(m.generation),
        37,
        sources.data[&m.generation].len(),
        cp,
        MAX_ENTRIES,
    )
    .unwrap();
    let mut failed = false;
    for _ in 0..10000 {
        match c.advance(DescriptorCursorBudget {
            bytes: 97,
            entries: 1,
        }) {
            Err(_) => {
                failed = true;
                break;
            }
            Ok(p) => {
                assert!(!p.ready, "corrupt cursor admitted");
                if let Some(origin) = p.needed_origin {
                    sources.supply(&mut c, origin);
                }
            }
        }
    }
    assert!(failed);
    assert!(
        c.advance(DescriptorCursorBudget {
            bytes: 1,
            entries: 1
        })
        .is_err()
    );
    assert!(c.finish().is_err());
}
fn root_rehash(bytes: &mut [u8]) {
    let r = NodeRef::decode(&bytes[192..256]).unwrap();
    let at = r.offset as usize;
    let hash: [u8; 32] = Sha256::digest(&bytes[at..at + r.length as usize]).into();
    bytes[224..256].copy_from_slice(&hash);
}

#[test]
fn indexed_cursor_rejects_same_authenticated_corruption_as_strict_decoder() {
    let h = history(9);
    let m = h.last().unwrap();
    let mut sources = Sources::new(&h);
    let original = sources.data[&m.generation].clone();
    let mut mutations = Vec::new();
    for (at, len) in [
        (8, 2),
        (16, 16),
        (32, 16),
        (48, 8),
        (64, 16),
        (176, 8),
        (184, 8),
    ] {
        let mut b = original.clone();
        b[at..at + len].fill(0);
        if at == 8 {
            b[at] = 1;
        }
        mutations.push(b);
    }
    let mut count = original.clone();
    count[112..116].copy_from_slice(&((m.segments.len() + 1) as u32).to_le_bytes());
    mutations.push(count);
    let mut cycle = original.clone();
    let r = NodeRef::decode(&cycle[192..256]).unwrap();
    let at = r.offset as usize;
    let left: [u8; 64] = number(&cycle, at + 32).unwrap();
    cycle[at + 96..at + 160].copy_from_slice(&left);
    root_rehash(&mut cycle);
    mutations.push(cycle);
    let mut geometry = original.clone();
    geometry[at + 16] = 64;
    root_rehash(&mut geometry);
    mutations.push(geometry);
    let mut extra = original.clone();
    let local = u32::from_le_bytes(number(&extra, 116).unwrap());
    extra[116..120].copy_from_slice(&(local + 1).to_le_bytes());
    let leaf = extra[256..352].to_vec();
    extra.extend_from_slice(&leaf);
    mutations.push(extra);
    for bad in mutations {
        assert!(strict(&bad, &sources).is_err());
        reject(bad, m, &mut sources, true);
        sources.replace(m.generation, original.clone());
    }
    let mut wrong_hash = original.clone();
    wrong_hash[HEADER + 48] ^= 1;
    reject(wrong_hash, m, &mut sources, false);
    sources.replace(m.generation, original);
    // An origin's node hash is still checked even though selected SHA is valid.
    let origin = h[0].generation;
    let mut old = sources.data[&origin].clone();
    old[HEADER + 48] ^= 1;
    sources.replace(origin, old);
    assert!(strict(&sources.data[&m.generation], &sources).is_err());
    let selected = sources.data[&m.generation].clone();
    reject(selected, m, &mut sources, true);
}

#[test]
fn indexed_cursor_missing_changed_io_and_creator_fail_closed() {
    let h = history(3);
    let m = h.last().unwrap();
    let sources = Sources::new(&h);
    let mut limit = IndexedDescriptorCursor::new(
        sources.file(m.generation),
        37,
        sources.data[&m.generation].len(),
        crate::checkpoint_bytes(m).unwrap(),
        2,
    )
    .unwrap();
    assert!(matches!(
        limit.advance(DescriptorCursorBudget {
            bytes: HEADER,
            entries: 1
        }),
        Err(Error::LimitExceeded)
    ));
    assert!(limit.finish().is_err());
    let mut missing = sources.cursor(m);
    let requested = loop {
        let p = missing
            .advance(DescriptorCursorBudget {
                bytes: 4096,
                entries: 20,
            })
            .unwrap();
        if let Some(o) = p.needed_origin {
            break o;
        }
        assert!(!p.ready);
    };
    let p = missing
        .advance(DescriptorCursorBudget {
            bytes: 4096,
            entries: 20,
        })
        .unwrap();
    assert_eq!(
        (p.read_bytes, p.work_units, p.needed_origin),
        (0, 0, Some(requested))
    );
    assert!(missing.finish().is_err());
    let mut changed = sources.cursor(m);
    changed
        .advance(DescriptorCursorBudget {
            bytes: 17,
            entries: 1,
        })
        .unwrap();
    OpenOptions::new()
        .write(true)
        .open(sources.path(m.generation))
        .unwrap()
        .set_len(1)
        .unwrap();
    assert!(
        changed
            .advance(DescriptorCursorBudget {
                bytes: 1,
                entries: 1
            })
            .is_err()
    );
    assert!(changed.finish().is_err());
    let sources = Sources::new(&h);
    let write = OpenOptions::new()
        .write(true)
        .open(sources.path(m.generation))
        .unwrap();
    let mut io = IndexedDescriptorCursor::new(
        write,
        37,
        sources.data[&m.generation].len(),
        crate::checkpoint_bytes(m).unwrap(),
        MAX_ENTRIES,
    )
    .unwrap();
    assert!(matches!(
        io.advance(DescriptorCursorBudget {
            bytes: 1,
            entries: 1
        }),
        Err(Error::Io(_))
    ));
    assert!(io.finish().is_err());
    let mut inherited = sources.cursor(m);
    inherited.creator = inherited.creator.wrapping_add(1);
    assert!(matches!(
        inherited.advance(DescriptorCursorBudget {
            bytes: 1,
            entries: 1
        }),
        Err(Error::InvalidInput("inherited indexed descriptor cursor"))
    ));
    assert!(inherited.finish().is_err());
}
