//! Read-only, bounded record pages over one immutable ULMDB snapshot.
use std::{
    net::IpAddr,
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Prefix, ReadTxn};

pub const SCAN_LIMIT: usize = 4096;
pub const VALUE_PREVIEW_LIMIT: usize = 4096;
pub const MAX_PAGE_SIZE: usize = 256;
pub const MAX_QUERY_BYTES: usize = 1024;

#[derive(Clone, Debug)]
pub struct Database {
    pub name: Vec<u8>,
    pub kind: DatabaseKind,
}

/// Continuations belong to a specific snapshot, database, and search.
#[derive(Clone, Debug)]
pub struct Position {
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>,
    pub prefix: Option<Prefix>,
    commit: u128,
    database: Vec<u8>,
    query: String,
}

#[derive(Clone, Debug)]
pub struct Record {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub value_len: usize,
    /// Exact identity for a duplicate whose value exceeds the displayed preview.
    /// Duplicate values are bounded by the engine at 64 KiB.
    pub duplicate_value: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct Page {
    pub records: Vec<Record>,
    pub resume: Option<Position>,
    pub complete: bool,
    pub scanned: usize,
}

pub struct Reader {
    path: PathBuf,
    read: ReadTxn,
    databases: Vec<Database>,
}

impl Reader {
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = environment_path(path)?;
        if path.join("RESIDENT.wal").is_file() {
            return Err("Resident environments cannot yet be browsed read-only. Export an immutable file snapshot, then open that snapshot here.".into());
        }
        let read = ReadTxn::open_snapshot(&path)
            .map_err(|e| format!("Could not open a read-only snapshot: {e}"))?;
        let databases = read
            .database_names()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|name| {
                read.database_kind(&name)
                    .map(|kind| Database { name, kind })
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            path,
            read,
            databases,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn sequence(&self) -> u64 {
        self.read.sequence()
    }
    pub fn databases(&self) -> &[Database] {
        &self.databases
    }

    /// Internal navigation anchors may be rebound only after a confirmed local
    /// mutation. Public page continuations retain their strict commit checks.
    pub(crate) fn check_mutation_refresh(
        &self,
        uuid: [u8; 16],
        sequence: u64,
    ) -> Result<(), String> {
        if self.read.file_manifest().map_err(|e| e.to_string())?.uuid != uuid
            || self.read.sequence() < sequence
        {
            return Err(
                "The database was replaced or rolled back while refreshing the change.".into(),
            );
        }
        Ok(())
    }

    pub(crate) fn rebind_anchor(
        &self,
        position: &Position,
        old_commit: u128,
    ) -> Result<Position, String> {
        if position.commit != old_commit {
            return Err(
                "The navigation anchor belongs to a different snapshot; refresh first.".into(),
            );
        }
        let mut position = position.clone();
        position.commit = self.read.commit_identity();
        Ok(position)
    }

    pub(crate) fn matches_value(query: &str, key: &[u8], value: &[u8]) -> Result<bool, String> {
        Ok(Search::parse(query)?.matches(key, &value[..value.len().min(VALUE_PREVIEW_LIMIT)]))
    }

    /// Find a small window before an edited ordered pair without rescanning
    /// earlier pages. Reverse work is capped at the ordinary page scan budget.
    pub(crate) fn anchor_around(
        &self,
        database: &[u8],
        key: &[u8],
        value: Option<&[u8]>,
        query: &str,
        before: usize,
    ) -> Result<Option<Position>, String> {
        let search = Search::parse(query)?;
        let kind = self
            .read
            .database_kind(database)
            .map_err(|e| e.to_string())?;
        if !matches!(kind, DatabaseKind::Kv | DatabaseKind::Duplicates { .. }) {
            return Err("Only ordered key/value records can move between pages.".into());
        }
        let duplicate = matches!(kind, DatabaseKind::Duplicates { .. });
        let mut bound = (key.to_vec(), value.map(<[u8]>::to_vec));
        let mut matches = 0;
        let mut anchor = None;
        for _ in 0..SCAN_LIMIT {
            let pair = self
                .read
                .client_seek_pair_borrowed(
                    database,
                    Some((&bound.0, bound.1.as_deref())),
                    false,
                    true,
                )
                .map_err(|e| e.to_string())?;
            let Some((key, value)) = pair else {
                return Ok(None);
            };
            let position = Position {
                key: key.to_vec(),
                value: duplicate.then(|| value.to_vec()),
                prefix: None,
                commit: self.read.commit_identity(),
                database: database.to_vec(),
                query: query.to_owned(),
            };
            if search.matches(key, &value[..value.len().min(VALUE_PREVIEW_LIMIT)]) {
                matches += 1;
            }
            bound = (position.key.clone(), position.value.clone());
            anchor = Some(position);
            if matches > before {
                break;
            }
        }
        Ok(anchor)
    }

    /// Resolve the complete selected value from this pinned snapshot. Mutation
    /// targets never contain an incomplete value copied from a display preview.
    pub fn target(
        &self,
        database: &[u8],
        record: &Record,
    ) -> Result<crate::mutation::Target, String> {
        if record.value_len > crate::mutation::MAX_EDIT_BYTES {
            return Err("This record is larger than the 64 KiB mutation limit; use the owning application to change it.".into());
        }
        let kind = self
            .read
            .database_kind(database)
            .map_err(|e| e.to_string())?;
        if matches!(kind, DatabaseKind::Kv | DatabaseKind::Duplicates { .. }) {
            let ordering = self
                .read
                .database_ordering(database)
                .map_err(|e| e.to_string())?;
            if ordering.keys == ulmdb::ordering::OrderingSpec::Custom
                || ordering.duplicates == ulmdb::ordering::OrderingSpec::Custom
            {
                return Err(
                    "Application-defined comparators cannot be edited by the explorer.".into(),
                );
            }
        }
        let duplicate = if matches!(kind, DatabaseKind::Duplicates { .. }) {
            Some(if record.value_len > VALUE_PREVIEW_LIMIT {
                record
                    .duplicate_value
                    .as_deref()
                    .ok_or("The full duplicate identity is missing; refresh first.")?
            } else {
                record.value.as_slice()
            })
        } else {
            None
        };
        let value = crate::mutation::value(&self.read, kind, database, &record.key, duplicate)
            .map_err(|e| e.to_string())?
            .ok_or("The selected record is missing; refresh first.")?;
        if value.len() != record.value_len
            || value.get(..value.len().min(VALUE_PREVIEW_LIMIT)) != Some(record.value.as_slice())
        {
            return Err("The selected record no longer matches its preview; refresh first.".into());
        }
        Ok(crate::mutation::Target {
            database: database.to_vec(),
            kind,
            key: record.key.clone(),
            old_value: value.to_vec(),
            commit: self.read.commit_identity(),
            uuid: self.read.file_manifest().map_err(|e| e.to_string())?.uuid,
        })
    }

    pub fn page(
        &self,
        database: &[u8],
        after: Option<&Position>,
        query: &str,
        limit: usize,
    ) -> Result<Page, String> {
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(format!("Page size must be between 1 and {MAX_PAGE_SIZE}."));
        }
        if query.len() > MAX_QUERY_BYTES {
            return Err(format!(
                "Search is limited to {MAX_QUERY_BYTES} UTF-8 bytes."
            ));
        }
        if let Some(after) = after
            && (after.commit != self.read.commit_identity()
                || after.database != database
                || after.query != query)
        {
            return Err("This page belongs to a different snapshot, database, or search. Start again from the first page.".into());
        }
        let kind = self
            .read
            .database_kind(database)
            .map_err(|e| e.to_string())?;
        if matches!(kind, DatabaseKind::Kv | DatabaseKind::Duplicates { .. }) {
            let order = self
                .read
                .database_ordering(database)
                .map_err(|e| e.to_string())?;
            if order.keys == ulmdb::ordering::OrderingSpec::Custom
                || order.duplicates == ulmdb::ordering::OrderingSpec::Custom
            {
                return Err("This database uses an application-defined comparator. Browse it through the owning application or export it with that comparator loaded.".into());
            }
        }
        let search = Search::parse(query)?;
        let mut page = Page {
            records: Vec::new(),
            resume: after.cloned(),
            complete: false,
            scanned: 0,
        };
        let mut accept = |key: &[u8], value: &[u8], prefix: Option<Prefix>| {
            let preview = &value[..value.len().min(VALUE_PREVIEW_LIMIT)];
            let position = Position {
                key: key.to_vec(),
                value: matches!(kind, DatabaseKind::Duplicates { .. }).then(|| value.to_vec()),
                prefix,
                commit: self.read.commit_identity(),
                database: database.to_vec(),
                query: query.to_owned(),
            };
            page.scanned += 1;
            page.resume = Some(position.clone());
            if search.matches(key, preview) {
                page.records.push(Record {
                    key: key.to_vec(),
                    value: preview.to_vec(),
                    value_len: value.len(),
                    duplicate_value: (matches!(kind, DatabaseKind::Duplicates { .. })
                        && value.len() > VALUE_PREVIEW_LIMIT)
                        .then(|| value.to_vec()),
                });
            }
            page.scanned >= SCAN_LIMIT || page.records.len() >= limit
        };
        match kind {
            DatabaseKind::Kv | DatabaseKind::Duplicates { .. } => {
                let mut position = after.map(|p| (p.key.clone(), p.value.clone()));
                loop {
                    let bound = position
                        .as_ref()
                        .map(|(key, value)| (key.as_slice(), value.as_deref()));
                    let pair = self
                        .read
                        .client_seek_pair_borrowed(database, bound, position.is_none(), false)
                        .map_err(|e| format!("Could not read this database: {e}"))?;
                    let Some((key, value)) = pair else {
                        page.complete = true;
                        break;
                    };
                    let stop = accept(key, value, None);
                    position = Some((
                        key.to_vec(),
                        matches!(kind, DatabaseKind::Duplicates { .. }).then(|| value.to_vec()),
                    ));
                    if stop {
                        break;
                    }
                }
            }
            DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                let queries = prefix_remainder(kind, after.and_then(|p| p.prefix))?;
                let mut stopped = false;
                for query in queries {
                    let cursor = self
                        .read
                        .subnets(database, query)
                        .map_err(|e| e.to_string())?;
                    for row in cursor {
                        let (prefix, value) = row.map_err(|e| e.to_string())?;
                        if after.is_some_and(|p| p.prefix == Some(prefix)) {
                            continue;
                        }
                        let key = prefix_text(prefix);
                        if accept(key.as_bytes(), value, Some(prefix)) {
                            stopped = true;
                            break;
                        }
                    }
                    if stopped {
                        break;
                    }
                }
                page.complete = !stopped;
            }
        }
        if page.complete {
            page.resume = None;
        }
        Ok(page)
    }
}

/// Open native directories, CURRENT/segment files, and already-imported adapter
/// sidecars. Never import a foreign LMDB file or create anything on disk.
pub fn environment_path(selected: &Path) -> Result<PathBuf, String> {
    let selected = std::fs::canonicalize(selected)
        .map_err(|e| format!("Could not find {}: {e}", selected.display()))?;
    let valid = |p: &Path| {
        p.join("CURRENT").is_file()
            || p.join("RANGE.pack").is_file()
            || p.join("RESIDENT.wal").is_file()
    };
    if selected.is_dir() {
        if valid(&selected) {
            return Ok(selected);
        }
        let sidecar = selected.join(".ulmdb");
        if valid(&sidecar) {
            return Ok(sidecar);
        }
    } else {
        let mut sidecar = selected.as_os_str().to_owned();
        sidecar.push(".ulmdb");
        let sidecar = PathBuf::from(sidecar);
        if valid(&sidecar) {
            return Ok(sidecar);
        }
        if let Some(parent) = selected.parent() {
            if valid(parent) {
                return Ok(parent.to_path_buf());
            }
            let sidecar = parent.join(".ulmdb");
            if valid(&sidecar) {
                return Ok(sidecar);
            }
        }
    }
    Err("Select a ULMDB environment directory, its CURRENT/segment file, or an already-imported LMDB adapter file. Foreign LMDB files are not imported by the explorer.".into())
}

fn prefix_text(prefix: Prefix) -> String {
    let address = match prefix.network() {
        Address::V4(n) => IpAddr::V4(n.into()),
        Address::V6(n) => IpAddr::V6(n.into()),
    };
    format!("{address}/{}", prefix.length())
}

/// Partition the suffix after a prefix into disjoint radix subtrees. This
/// revisits at most the exact prior row, rather than rescanning old pages.
fn prefix_remainder(kind: DatabaseKind, after: Option<Prefix>) -> Result<Vec<Prefix>, String> {
    let zero = match kind {
        DatabaseKind::Ipv4 => Address::V4(0),
        DatabaseKind::Ipv6 => Address::V6(0),
        _ => return Err("This is not a prefix database.".into()),
    };
    let Some(after) = after else {
        return Ok(vec![Prefix::new(zero, 0).unwrap()]);
    };
    if std::mem::discriminant(&zero) != std::mem::discriminant(&after.network()) {
        return Err("Continuation has a different address family.".into());
    }
    let mut queries = vec![after];
    for length in (1..=after.length()).rev() {
        let next = match after.network() {
            Address::V4(n) => {
                let bit = 1u32 << (32 - length);
                (n & bit == 0).then(|| Address::V4((n & (u32::MAX << (32 - length))) | bit))
            }
            Address::V6(n) => {
                let bit = 1u128 << (128 - length);
                (n & bit == 0).then(|| Address::V6((n & (u128::MAX << (128 - length))) | bit))
            }
        };
        if let Some(next) = next {
            queries.push(Prefix::new(next, length).unwrap());
        }
    }
    Ok(queries)
}

enum Search {
    Text(String),
    Bytes(Vec<u8>),
}
impl Search {
    fn parse(query: &str) -> Result<Self, String> {
        if let Some(hex) = query.strip_prefix("hex:") {
            let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
            if !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(
                    "Hex search needs pairs of hexadecimal digits, for example hex:00ff.".into(),
                );
            }
            let bytes = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            Ok(Self::Bytes(bytes))
        } else {
            Ok(Self::Text(query.to_lowercase()))
        }
    }
    fn matches(&self, key: &[u8], value: &[u8]) -> bool {
        match self {
            Self::Text(query) => {
                query.is_empty()
                    || [key, value].iter().any(|bytes| {
                        String::from_utf8_lossy(bytes)
                            .to_lowercase()
                            .contains(query)
                    })
            }
            Self::Bytes(query) => {
                query.is_empty()
                    || [key, value]
                        .iter()
                        .any(|bytes| bytes.windows(query.len()).any(|window| window == query))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicU64, Ordering},
    };
    use ulmdb::Environment;

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                    "ulmdb-explorer-{}-{}-{n}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )))
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn cidr(text: &str) -> Prefix {
        let (ip, length) = text.split_once('/').unwrap();
        Prefix::new(
            ip.parse::<IpAddr>().unwrap().into(),
            length.parse().unwrap(),
        )
        .unwrap()
    }
    fn collect(reader: &Reader, database: &[u8], query: &str, limit: usize) -> Vec<Record> {
        let mut rows = Vec::new();
        let mut after = None;
        for _ in 0..100 {
            let page = reader.page(database, after.as_ref(), query, limit).unwrap();
            assert!(page.records.len() <= limit);
            assert!(page.scanned <= SCAN_LIMIT);
            rows.extend(page.records);
            if page.complete {
                return rows;
            }
            assert!(page.resume.is_some());
            after = page.resume;
        }
        panic!("pagination did not terminate")
    }
    fn files(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut result = BTreeMap::new();
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                result.extend(files(&path));
            } else {
                result.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
        result
    }

    #[test]
    fn all_database_kinds_page_without_skips_and_refresh_preserves_snapshot() {
        let dir = Directory::new();
        let env = Environment::create(&dir.0).unwrap();
        let mut w = env.begin_write().unwrap();
        for (name, kind) in [
            (b"kv".as_slice(), DatabaseKind::Kv),
            (b"dup", DatabaseKind::Duplicates { width: None }),
            (b"v4", DatabaseKind::Ipv4),
            (b"v6", DatabaseKind::Ipv6),
        ] {
            w.create_database(name, kind).unwrap();
        }
        for n in 0..10 {
            w.put(
                b"kv",
                format!("key-{n:02}").as_bytes(),
                format!("VALUE-{n:02}").as_bytes(),
            )
            .unwrap();
            for v in 0..3 {
                w.insert_duplicate(
                    b"dup",
                    format!("k{n}").as_bytes(),
                    format!("v{v}").as_bytes(),
                )
                .unwrap();
            }
        }
        let v4 = [
            "0.0.0.0/0",
            "0.0.0.0/1",
            "0.0.0.0/32",
            "10.0.0.0/8",
            "10.0.0.0/16",
            "10.0.0.1/32",
            "64.0.0.0/2",
            "128.0.0.0/1",
            "255.255.255.255/32",
        ];
        let v6 = [
            "::/0",
            "::/1",
            "::/128",
            "::1/128",
            "2001:db8::/32",
            "2001:db8::/64",
            "2001:db8::1/128",
            "8000::/1",
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/128",
        ];
        for text in v4 {
            w.insert_prefix(b"v4", cidr(text), text.as_bytes()).unwrap();
        }
        for text in v6 {
            w.insert_prefix(b"v6", cidr(text), text.as_bytes()).unwrap();
        }
        w.commit().unwrap();
        let before = files(&dir.0);
        let reader = Reader::open(&dir.0.join("CURRENT")).unwrap();
        assert_eq!(reader.databases().len(), 4);
        assert_eq!(collect(&reader, b"kv", "", 3).len(), 10);
        assert_eq!(collect(&reader, b"dup", "", 2).len(), 30);
        for (name, expected) in [(b"v4".as_slice(), v4.as_slice()), (b"v6", v6.as_slice())] {
            let actual: Vec<_> = collect(&reader, name, "", 1)
                .into_iter()
                .map(|r| String::from_utf8(r.key).unwrap())
                .collect();
            assert_eq!(actual, expected);
        }
        assert_eq!(collect(&reader, b"kv", "value-03", 2).len(), 1);
        assert_eq!(
            before,
            files(&dir.0),
            "opening and browsing must not modify source bytes"
        );
        let first = reader.page(b"kv", None, "", 2).unwrap();
        assert!(reader.page(b"dup", first.resume.as_ref(), "", 2).is_err());
        assert!(
            reader
                .page(b"kv", first.resume.as_ref(), "changed search", 2)
                .is_err()
        );
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"new", b"new value").unwrap();
        w.commit().unwrap();
        let refreshed = Reader::open(&dir.0).unwrap();
        assert!(refreshed.sequence() > reader.sequence());
        assert_eq!(collect(&reader, b"kv", "", 3).len(), 10);
        assert_eq!(collect(&refreshed, b"kv", "", 3).len(), 11);
        assert!(refreshed.page(b"kv", first.resume.as_ref(), "", 2).is_err());
    }

    #[test]
    fn search_has_explicit_scan_and_preview_bounds_and_binary_search() {
        let dir = Directory::new();
        let env = Environment::create(&dir.0).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for n in 0..SCAN_LIMIT + 2 {
            w.put(b"kv", format!("k{n:05}").as_bytes(), b"haystack")
                .unwrap();
        }
        let mut large = vec![b'a'; VALUE_PREVIEW_LIMIT + 10];
        large[0] = 0;
        large[1] = 255;
        large[VALUE_PREVIEW_LIMIT..].copy_from_slice(b"only-atend");
        w.put(b"kv", b"zzz", &large).unwrap();
        w.commit().unwrap();
        let reader = Reader::open(&dir.0).unwrap();
        let first = reader.page(b"kv", None, "missing", 20).unwrap();
        assert!(first.records.is_empty());
        assert!(!first.complete);
        assert_eq!(first.scanned, SCAN_LIMIT);
        let last = reader
            .page(b"kv", first.resume.as_ref(), "missing", 20)
            .unwrap();
        assert!(last.complete);
        assert_eq!(last.scanned, 3);
        let binary = collect(&reader, b"kv", "hex:00 ff", 20);
        assert_eq!(binary.len(), 1);
        assert_eq!(binary[0].value.len(), VALUE_PREVIEW_LIMIT);
        assert_eq!(binary[0].value_len, large.len());
        assert!(collect(&reader, b"kv", "only-atend", 20).is_empty());
        assert!(reader.page(b"kv", None, "hex:0zz", 20).is_err());
        assert!(reader.page(b"kv", None, "", MAX_PAGE_SIZE + 1).is_err());
    }

    #[test]
    fn adapter_sidecars_are_discovered_without_import_or_creation() {
        let dir = Directory::new();
        std::fs::create_dir(&dir.0).unwrap();
        let marker = dir.0.join("data.mdb");
        std::fs::write(&marker, b"foreign bytes").unwrap();
        assert!(Reader::open(&marker).is_err());
        assert!(!dir.0.join(".ulmdb").exists());
        let _env = Environment::create(dir.0.join(".ulmdb")).unwrap();
        let native = std::fs::canonicalize(dir.0.join(".ulmdb")).unwrap();
        assert_eq!(Reader::open(&dir.0).unwrap().path(), native);
        assert_eq!(Reader::open(&marker).unwrap().path(), native);
        let nosubdir = dir.0.join("single.db");
        std::fs::write(&nosubdir, b"marker").unwrap();
        let sidecar = dir.0.join("single.db.ulmdb");
        let _single = Environment::create(&sidecar).unwrap();
        assert_eq!(Reader::open(&nosubdir).unwrap().path(), sidecar);
    }

    #[test]
    fn prefix_suffix_partition_matches_network_then_length_order() {
        // Exercise nested, adjacent and empty subtrees against an independent
        // ordered model; the eight-bit family exhausts every prefix shape.
        let mut prefixes = Vec::new();
        for length in 0..=8 {
            for n in 0..(1u32 << length) {
                let network = if length == 0 { 0 } else { n << (32 - length) };
                prefixes.push(Prefix::new(Address::V4(network), length).unwrap());
            }
        }
        prefixes.sort_unstable();
        for &after in &prefixes {
            let queries = prefix_remainder(DatabaseKind::Ipv4, Some(after)).unwrap();
            let actual: Vec<_> = queries
                .iter()
                .flat_map(|query| prefixes.iter().filter(|p| query.contains_prefix(**p)))
                .copied()
                .filter(|p| *p != after)
                .collect();
            let expected: Vec<_> = prefixes.iter().copied().filter(|p| *p > after).collect();
            assert_eq!(actual, expected, "after {after:?}");
        }
    }

    #[test]
    fn builtin_ordering_is_respected_and_custom_comparators_are_explicit() {
        use ulmdb::ordering::{DatabaseOrdering, OrderingSpec};
        let dir = Directory::new();
        let env = Environment::create(&dir.0).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"reverse", DatabaseKind::Kv).unwrap();
        w.set_database_ordering(
            b"reverse",
            DatabaseOrdering {
                keys: OrderingSpec::ReverseBytes,
                ..Default::default()
            },
        )
        .unwrap();
        for key in [b"za", b"ab", b"bc"] {
            w.put(b"reverse", key, key).unwrap();
        }
        w.create_database(b"custom", DatabaseKind::Kv).unwrap();
        w.set_database_ordering(
            b"custom",
            DatabaseOrdering {
                keys: OrderingSpec::Custom,
                ..Default::default()
            },
        )
        .unwrap();
        w.commit().unwrap();
        let reader = Reader::open(&dir.0).unwrap();
        let keys: Vec<_> = collect(&reader, b"reverse", "", 1)
            .into_iter()
            .map(|r| r.key)
            .collect();
        assert_eq!(keys, [b"za", b"ab", b"bc"]);
        let error = reader.page(b"custom", None, "", 20).unwrap_err();
        assert!(error.contains("application-defined comparator"));
    }

    #[test]
    fn cohort_and_indexed_are_browsed_without_source_changes() {
        for mode in [
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let dir = Directory::new();
            let env =
                Environment::create_with_residency(&dir.0, mode, ulmdb::ResidencyPolicy::Demand)
                    .unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"records", DatabaseKind::Kv).unwrap();
            w.put(b"records", b"first", b"One").unwrap();
            w.put(b"records", b"second", b"Two").unwrap();
            w.commit().unwrap();
            let before = files(&dir.0);
            let reader = Reader::open(&dir.0).unwrap();
            assert_eq!(reader.databases()[0].name, b"records");
            assert_eq!(collect(&reader, b"records", "", 1).len(), 2);
            assert_eq!(collect(&reader, b"records", "two", 1)[0].key, b"second");
            assert_eq!(
                files(&dir.0),
                before,
                "{mode:?} must remain byte-for-byte unchanged"
            );
        }
    }

    #[test]
    fn unsupported_range_open_does_not_recover_or_modify_source() {
        for mode in [
            ulmdb::PublicationMode::Range,
            ulmdb::PublicationMode::RangeV5,
        ] {
            let dir = Directory::new();
            let _env =
                Environment::create_with_residency(&dir.0, mode, ulmdb::ResidencyPolicy::Demand)
                    .unwrap();
            let before = files(&dir.0);
            let error = Reader::open(&dir.0)
                .err()
                .expect("the explorer enables no Range capture feature");
            assert!(error.contains("read-only snapshot"));
            assert_eq!(
                files(&dir.0),
                before,
                "unsupported {mode:?} must remain unchanged"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unsupported_resident_open_does_not_negotiate_capture_or_modify_source() {
        let dir = Directory::new();
        let mut options = ulmdb::resident::Options::default();
        options.pool.kv_pages = 128;
        options.pool.tiny_pages = 64;
        options.pool.max_reserved_bytes = 32 << 20;
        options.wal_capacity = 1 << 20;
        let env = ulmdb::resident::Environment::create(&dir.0, options).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"records", DatabaseKind::Kv).unwrap();
        w.put(b"records", b"key", b"value").unwrap();
        w.commit().unwrap();
        let before = files(&dir.0);
        let error = Reader::open(&dir.0)
            .err()
            .expect("Resident has no generic catalog explorer yet");
        assert!(error.contains("Resident"));
        assert!(error.contains("snapshot"));
        assert_eq!(files(&dir.0), before);
        assert!(!dir.0.join("RESIDENT-CAPTURE").exists());
    }
}
