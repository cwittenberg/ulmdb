use std::{collections::BTreeSet, ops::Bound::Unbounded, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, Error};

type Pair = (Vec<u8>, Vec<u8>);
type Bound<'a> = Option<(&'a [u8], Option<&'a [u8]>)>;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-client-cursor-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn expected(
    pairs: &[Pair],
    bound: Bound<'_>,
    inclusive: bool,
    reverse: bool,
    duplicate: bool,
) -> Option<Pair> {
    let accepts = |(key, value): &&Pair| match bound {
        None => true,
        Some((bound_key, bound_value)) => {
            let ordering = if duplicate {
                match bound_value {
                    Some(bound_value) => {
                        (key.as_slice(), value.as_slice()).cmp(&(bound_key, bound_value))
                    }
                    None => key.as_slice().cmp(bound_key),
                }
            } else {
                key.as_slice().cmp(bound_key)
            };
            (if reverse {
                ordering.is_lt()
            } else {
                ordering.is_gt()
            }) || (inclusive && ordering.is_eq())
        }
    };
    if reverse {
        pairs.iter().rev().find(accepts).cloned()
    } else {
        pairs.iter().find(accepts).cloned()
    }
}

fn assert_seeks(
    pairs: &[Pair],
    duplicate: bool,
    seek: impl Fn(Bound<'_>, bool, bool) -> ulmdb::Result<Option<Pair>>,
) {
    let bounds: Vec<Bound<'_>> = vec![
        None,
        Some((b"", None)),
        Some((b"k00", None)),
        Some((b"k01", None)),
        Some((b"k01x", None)),
        Some((b"k04", Some(b"v01"))),
        Some((b"k04", Some(b"v01x"))),
        Some((b"k03", Some(b""))),
        Some((b"k02", Some(b"v00"))),
        Some((b"k29", Some(b"\xff"))),
        Some((b"zz", None)),
    ];
    for bound in bounds {
        for inclusive in [false, true] {
            for reverse in [false, true] {
                assert_eq!(
                    seek(bound, inclusive, reverse).unwrap(),
                    expected(pairs, bound, inclusive, reverse, duplicate),
                    "bound={bound:?}, inclusive={inclusive}, reverse={reverse}, duplicate={duplicate}"
                );
            }
        }
    }
    for reverse in [false, true] {
        let mut current = seek(None, true, reverse).unwrap();
        let mut actual = Vec::new();
        while let Some(pair) = current {
            current = seek(Some((&pair.0, Some(&pair.1))), false, reverse).unwrap();
            actual.push(pair);
        }
        let expected: Vec<_> = if reverse {
            pairs.iter().rev().cloned().collect()
        } else {
            pairs.to_vec()
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn bounded_pair_seeks_match_models_and_existing_cursors_for_readers_and_writers() {
    let directory = Directory::new();
    let environment = Environment::create(&directory.0).unwrap();
    let mut writer = environment.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer
        .create_database(b"duplicates", DatabaseKind::Duplicates { width: None })
        .unwrap();
    let mut kv = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for number in 0..30 {
        let key = format!("k{number:02}").into_bytes();
        let value = format!("value-{number:02}").into_bytes();
        writer.put(b"kv", &key, &value).unwrap();
        kv.insert((key.clone(), value));
        let count = if number % 3 == 0 { 3 } else { 12 };
        for value in 0..count {
            let bytes = format!("v{value:02}").into_bytes();
            writer
                .insert_duplicate(b"duplicates", &key, &bytes)
                .unwrap();
            duplicates.insert((key.clone(), bytes));
        }
    }
    // A large value forces a subtree even when the duplicate count is small.
    let large = vec![b'z'; 700];
    writer
        .insert_duplicate(b"duplicates", b"k02", &large)
        .unwrap();
    duplicates.insert((b"k02".to_vec(), large));
    let kv: Vec<_> = kv.into_iter().collect();
    let duplicates: Vec<_> = duplicates.into_iter().collect();
    assert_seeks(&kv, false, |bound, inclusive, reverse| {
        writer.client_seek_pair(b"kv", bound, inclusive, reverse)
    });
    assert_seeks(&duplicates, true, |bound, inclusive, reverse| {
        writer.client_seek_pair(b"duplicates", bound, inclusive, reverse)
    });
    writer.commit().unwrap();
    let reader = environment.begin_read();
    assert_seeks(&kv, false, |bound, inclusive, reverse| {
        reader.client_seek_pair(b"kv", bound, inclusive, reverse)
    });
    assert_seeks(&duplicates, true, |bound, inclusive, reverse| {
        reader.client_seek_pair(b"duplicates", bound, inclusive, reverse)
    });
    let full_kv: Vec<_> = reader
        .range(b"kv", Unbounded, Unbounded, false)
        .unwrap()
        .map(|pair| {
            let (key, value) = pair.unwrap();
            (key.to_vec(), value.to_vec())
        })
        .collect();
    let full_duplicates: Vec<_> = reader
        .duplicate_pairs(b"duplicates", Unbounded, Unbounded, false)
        .unwrap()
        .map(|pair| {
            let (key, value) = pair.unwrap();
            (key.to_vec(), value.to_vec())
        })
        .collect();
    assert_eq!(full_kv, kv);
    assert_eq!(full_duplicates, duplicates);
}

#[test]
fn seeks_cover_empty_databases_fixed_duplicates_private_mutations_and_validation() {
    let directory = Directory::new();
    let environment = Environment::create(&directory.0).unwrap();
    let mut writer = environment.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer
        .create_database(b"duplicates", DatabaseKind::Duplicates { width: Some(4) })
        .unwrap();
    writer
        .create_database(b"prefix", DatabaseKind::Ipv4)
        .unwrap();
    for name in [b"kv".as_slice(), b"duplicates".as_slice()] {
        for reverse in [false, true] {
            assert!(
                writer
                    .client_seek_pair(name, None, true, reverse)
                    .unwrap()
                    .is_none()
            );
        }
    }
    for value in [b"0000", b"1111", b"2222"] {
        writer
            .insert_duplicate(b"duplicates", b"key", value)
            .unwrap();
    }
    assert_eq!(
        writer
            .client_seek_pair(b"duplicates", Some((b"key", Some(b"1111"))), false, false)
            .unwrap(),
        Some((b"key".to_vec(), b"2222".to_vec()))
    );
    let retained = writer
        .client_seek_pair(b"duplicates", None, true, false)
        .unwrap()
        .unwrap();
    writer
        .delete_duplicate(b"duplicates", b"key", b"0000")
        .unwrap();
    assert_eq!(retained.1, b"0000");
    assert_eq!(
        writer
            .client_seek_pair(b"duplicates", None, true, false)
            .unwrap()
            .unwrap()
            .1,
        b"1111"
    );
    assert!(matches!(
        writer.client_seek_pair(b"prefix", None, true, false),
        Err(Error::WrongKind)
    ));
    assert!(matches!(
        writer.client_seek_pair(b"missing", None, true, false),
        Err(Error::MissingDatabase)
    ));
    assert!(matches!(
        writer.client_seek_pair(b"kv", Some((&vec![0; 4097], None)), true, false),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        writer.client_seek_pair(
            b"duplicates",
            Some((b"key", Some(&vec![0; 65537]))),
            true,
            false
        ),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn borrowed_pairs_survive_other_seeks_and_commits_while_snapshot_is_pinned() {
    let directory = Directory::new();
    let environment = Environment::create(&directory.0).unwrap();
    let mut writer = environment.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer
        .create_database(b"duplicates", DatabaseKind::Duplicates { width: None })
        .unwrap();
    writer.put(b"kv", b"key", b"old-kv").unwrap();
    writer
        .insert_duplicate(b"duplicates", b"key", b"aa")
        .unwrap();
    writer
        .insert_duplicate(b"duplicates", b"key", b"bb")
        .unwrap();
    let private = writer
        .client_seek_pair_borrowed(b"kv", None, true, false)
        .unwrap()
        .unwrap();
    assert_eq!(private, (b"key".as_slice(), b"old-kv".as_slice()));
    writer.commit().unwrap();
    let snapshot = environment.begin_read();
    let old_kv = snapshot
        .client_seek_pair_borrowed(b"kv", None, true, false)
        .unwrap()
        .unwrap();
    let old_pair = snapshot
        .client_seek_pair_borrowed(b"duplicates", None, true, false)
        .unwrap()
        .unwrap();
    let next_pair = snapshot
        .client_seek_pair_borrowed(
            b"duplicates",
            Some((old_pair.0, Some(old_pair.1))),
            false,
            false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(next_pair.1, b"bb");
    let mut writer = environment.begin_write().unwrap();
    writer.put(b"kv", b"key", b"new-kv").unwrap();
    writer
        .delete_duplicate(b"duplicates", b"key", b"aa")
        .unwrap();
    writer
        .insert_duplicate(b"duplicates", b"key", b"cc")
        .unwrap();
    writer.commit().unwrap();
    assert_eq!(old_kv, (b"key".as_slice(), b"old-kv".as_slice()));
    assert_eq!(old_pair, (b"key".as_slice(), b"aa".as_slice()));
    assert_eq!(next_pair, (b"key".as_slice(), b"bb".as_slice()));
    let current = environment.begin_read();
    assert_eq!(
        current
            .client_seek_pair_borrowed(b"kv", None, true, false)
            .unwrap()
            .unwrap()
            .1,
        b"new-kv"
    );
    assert_eq!(
        current
            .client_seek_pair_borrowed(b"duplicates", None, true, false)
            .unwrap()
            .unwrap()
            .1,
        b"bb"
    );
}
