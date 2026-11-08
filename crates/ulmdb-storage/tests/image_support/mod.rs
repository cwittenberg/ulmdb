use std::{
    collections::BTreeMap,
    fs,
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb_core::{Address, Prefix, storage::Reference};
use ulmdb_storage::hybrid::{ChunkLimits, ImageTable};
pub struct Fixture(pub PathBuf);
impl Fixture {
    fn new(parent: &Path) -> Self {
        let p = parent.join(format!(
            "ulmdb-image-h1-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
pub fn filesystems(mut f: impl FnMut(Fixture)) {
    f(Fixture::new(&std::env::temp_dir()));
    if Path::new("/dev/shm").is_dir() {
        f(Fixture::new(Path::new("/dev/shm")));
    }
}
pub fn limits(capacity: usize) -> ChunkLimits {
    ChunkLimits {
        capacity,
        reserve_quantum: 65536,
        max_burned_bytes: capacity,
    }
}
pub fn kv_check(store: &ImageTable, root: Reference, model: &BTreeMap<Vec<u8>, Vec<u8>>) {
    for (k, v) in model {
        assert_eq!(ulmdb_kv::get(store, root, k).unwrap(), Some(v.as_slice()));
    }
    let got: Vec<_> = ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .map(|r| r.map(|(k, v)| (k.to_vec(), v.to_vec())))
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        got,
        model
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>()
    );
}
fn bits(a: Address) -> (u128, u8) {
    match a {
        Address::V4(n) => (u128::from(n), 32),
        Address::V6(n) => (n, 128),
    }
}
fn contains(network: Address, len: u8, address: Address) -> bool {
    let (a, w) = bits(network);
    let (b, w2) = bits(address);
    w == w2 && (0..len).all(|bit| ((a >> (w - 1 - bit)) & 1) == ((b >> (w - 1 - bit)) & 1))
}
pub fn radix_check(
    store: &ImageTable,
    root: Reference,
    records: &[(Prefix, Vec<u8>)],
    queries: &[Address],
) {
    for (p, v) in records {
        assert_eq!(
            ulmdb_prefix::exact(store, root, *p)
                .unwrap()
                .map(|(_, v)| v),
            Some(v.as_slice())
        );
    }
    for &q in queries {
        let e = records
            .iter()
            .filter(|(p, _)| contains(p.network(), p.length(), q))
            .max_by_key(|(p, _)| p.length());
        assert_eq!(
            ulmdb_prefix::get(store, root, q)
                .unwrap()
                .map(|(p, v)| (p, v.to_vec())),
            e.map(|(p, v)| (*p, v.clone()))
        );
    }
    if let Some((p, _)) = records.first() {
        let got: Vec<_> = ulmdb_prefix::subnets(store, root, *p)
            .unwrap()
            .map(|r| r.map(|(p, v)| (p, v.to_vec())))
            .collect::<Result<_, _>>()
            .unwrap();
        let mut e: Vec<_> = records
            .iter()
            .filter(|(q, _)| {
                q.length() >= p.length() && contains(p.network(), p.length(), q.network())
            })
            .cloned()
            .collect();
        e.sort_by_key(|(p, _)| (bits(p.network()).0, p.length()));
        assert_eq!(got, e);
    }
}
