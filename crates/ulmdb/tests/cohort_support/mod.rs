#![allow(dead_code)]
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Environment, Prefix, PublicationMode};
use ulmdb_storage::Manifest;

pub struct Directory(pub PathBuf);
impl Directory {
    pub fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-cohort-maintenance-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn cohort_environment(directory: &Directory) -> Environment {
    Environment::create_with_publication(&directory.0, PublicationMode::Cohort).unwrap()
}
pub fn cohort_path(directory: &Path, origin: u128) -> PathBuf {
    directory
        .join("cohorts")
        .join(format!("{origin:032x}.cohort"))
}
pub fn cohort_names(manifest: &Manifest) -> BTreeSet<u128> {
    let mut result = manifest
        .segments
        .iter()
        .filter_map(|entry| (entry.cohort != 0).then_some(entry.cohort))
        .collect::<BTreeSet<_>>();
    if manifest.format_minor >= 3 {
        result.insert(manifest.generation);
    }
    result
}
pub fn physical_paths(directory: &Path, manifest: &Manifest) -> BTreeSet<PathBuf> {
    let mut paths = manifest
        .segments
        .iter()
        .map(|entry| ulmdb_storage::segment_path(directory, entry))
        .collect::<BTreeSet<_>>();
    paths.insert(ulmdb_storage::descriptor_path(directory, manifest));
    paths
}
pub fn bytes_on_disk(directory: &Path, manifest: &Manifest) -> u64 {
    physical_paths(directory, manifest)
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum()
}
pub fn populate(environment: &Environment) {
    let mut writer = environment.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer
        .create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    writer
        .create_database(b"prefix", DatabaseKind::Ipv4)
        .unwrap();
    writer.put(b"kv", b"counter", &0u64.to_le_bytes()).unwrap();
    writer.put(b"kv", b"mirror", &0u64.to_le_bytes()).unwrap();
    writer.put(b"kv", b"large", &vec![0x71; 100_000]).unwrap();
    for n in 0..40u64 {
        writer
            .insert_duplicate(b"dups", b"key", &n.to_be_bytes())
            .unwrap();
    }
    writer
        .insert_prefix(
            b"prefix",
            Prefix::new(Address::V4(0x0a00_0000), 8).unwrap(),
            b"profile",
        )
        .unwrap();
    writer.commit().unwrap();
}
pub fn update(environment: &Environment, value: u64) {
    let mut writer = environment.begin_write().unwrap();
    writer.put(b"kv", b"counter", &value.to_le_bytes()).unwrap();
    writer.put(b"kv", b"mirror", &value.to_le_bytes()).unwrap();
    writer.commit().unwrap();
}
pub fn inspect(environment: &Environment, expected: u64) {
    let read = environment.begin_read();
    assert_eq!(
        read.get(b"kv", b"counter").unwrap(),
        Some(expected.to_le_bytes().as_slice())
    );
    assert_eq!(
        read.get(b"kv", b"counter").unwrap(),
        read.get(b"kv", b"mirror").unwrap()
    );
    assert_eq!(
        read.get(b"kv", b"large").unwrap().unwrap(),
        vec![0x71; 100_000]
    );
    assert_eq!(read.duplicate_count(b"dups", b"key").unwrap(), 40);
    assert_eq!(
        read.longest_match(b"prefix", Address::V4(0x0a00_0001))
            .unwrap()
            .unwrap()
            .1,
        b"profile"
    );
}
pub fn count(directory: &Path, child: &str) -> usize {
    fs::read_dir(directory.join(child)).unwrap().count()
}
pub fn same_entry_origins(before: &Manifest, after: &Manifest) {
    assert_eq!(before.format_minor, after.format_minor);
    assert_eq!(
        (
            before.uuid,
            before.generation,
            before.sequence,
            before.commit,
            before.parent,
            before.previous
        ),
        (
            after.uuid,
            after.generation,
            after.sequence,
            after.commit,
            after.parent,
            after.previous
        )
    );
    assert_eq!(before.segments.len(), after.segments.len());
    for (a, b) in before.segments.iter().zip(&after.segments) {
        assert_eq!(
            (a.id, a.length, a.hash, a.cohort),
            (b.id, b.length, b.hash, b.cohort)
        );
    }
}
