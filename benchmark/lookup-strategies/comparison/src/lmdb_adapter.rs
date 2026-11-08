//! Native LMDB adapter; NOTLS only permits a pinned reader alongside a writer.
//! Commit synchronization and normal file locking remain enabled.
use crate::{
    data::Record,
    protocol::{Lookup, Result},
};
use std::ops::Bound;
use ulmdb_lmdb_baseline::{Database, DatabaseKind, Environment, ReadDatabase, ReadTransaction};

pub struct Read<'txn, 'env> {
    transaction: &'txn ReadTransaction<'env>,
    database: &'txn Database<'env>,
    prepared: ReadDatabase<'txn>,
}

impl<'txn, 'env> Read<'txn, 'env> {
    pub fn new(
        transaction: &'txn ReadTransaction<'env>,
        database: &'txn Database<'env>,
    ) -> Result<Self> {
        Ok(Self {
            transaction,
            database,
            prepared: transaction.database(database)?,
        })
    }
}

fn above_lower(key: &[u8], bound: Bound<&[u8]>) -> bool {
    match bound {
        Bound::Unbounded => true,
        Bound::Included(k) => key >= k,
        Bound::Excluded(k) => key > k,
    }
}
fn below_upper(key: &[u8], bound: Bound<&[u8]>) -> bool {
    match bound {
        Bound::Unbounded => true,
        Bound::Included(k) => key <= k,
        Bound::Excluded(k) => key < k,
    }
}

impl Lookup for Read<'_, '_> {
    fn get(&self, key: &[u8]) -> Result<Option<&[u8]>> {
        Ok(self.prepared.get(key)?)
    }

    fn visit(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
        limit: usize,
        mut callback: impl FnMut(&[u8], &[u8]),
    ) -> Result<()> {
        if limit == 0 {
            return Ok(());
        }
        let mut cursor = self.transaction.cursor(self.database)?;
        #[derive(Clone, Copy)]
        enum Step<'a> {
            First,
            Last,
            Seek(&'a [u8]),
            Next,
            Previous,
        }
        let advance = if reverse { Step::Previous } else { Step::Next };
        let mut step = match if reverse { upper } else { lower } {
            Bound::Unbounded => {
                if reverse {
                    Step::Last
                } else {
                    Step::First
                }
            }
            Bound::Included(key) | Bound::Excluded(key) => Step::Seek(key),
        };
        let mut count = 0;
        while count < limit {
            // Consume each cursor borrow within one iteration. Positioning never
            // copies a key or repeats a successful SET_RANGE operation.
            let item = match step {
                Step::First => cursor.first()?,
                Step::Last => cursor.last()?,
                Step::Seek(key) => cursor.lower_bound(key)?,
                Step::Next => cursor.next_pair()?,
                Step::Previous => cursor.previous_pair()?,
            };
            let Some((key, value)) = item else {
                if reverse && matches!(step, Step::Seek(_)) {
                    step = Step::Last;
                    continue;
                }
                break;
            };
            if reverse {
                if !below_upper(key, upper) {
                    step = advance;
                    continue;
                }
                if !above_lower(key, lower) {
                    break;
                }
            } else {
                if !above_lower(key, lower) {
                    step = advance;
                    continue;
                }
                if !below_upper(key, upper) {
                    break;
                }
            }
            callback(key, value);
            count += 1;
            step = advance;
        }
        Ok(())
    }
}

pub fn load<'env>(environment: &'env Environment, rows: &[Record]) -> Result<Database<'env>> {
    let mut write = environment.begin_write()?;
    let database = write.open_database("kv", DatabaseKind::Ordinary, true)?;
    for row in rows {
        write.put(&database, &row.key, &row.value)?;
    }
    write.commit()?;
    Ok(database)
}

pub fn database(environment: &Environment) -> Result<Database<'_>> {
    let mut write = environment.begin_write()?;
    let database = write.open_database("kv", DatabaseKind::Ordinary, false)?;
    write.commit()?;
    Ok(database)
}

pub fn apply(
    environment: &Environment,
    database: &Database<'_>,
    rows: &[Record],
    changes: &[(usize, [u8; 32])],
) -> Result<()> {
    let mut write = environment.begin_write()?;
    for &(index, value) in changes {
        let row = rows.get(index).ok_or("change index outside fixture")?;
        write.put(database, &row.key, &value)?;
    }
    write.commit()?;
    Ok(())
}
