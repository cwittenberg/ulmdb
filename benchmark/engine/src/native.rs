//! Strategies are selected at the case boundary; lookup loops are monomorphic.
use crate::backend::Environment;
use crate::{
    Result,
    dataset::{Dataset, mask4, mask6},
    oracle::Answer,
};
use std::collections::BTreeSet;
use ulmdb::{Address, DatabaseKind, Prefix};
use ulmdb_lmdb_baseline::{
    Database, DatabaseKind as NativeKind, Environment as NativeEnvironment, ReadDatabase,
    ReadTransaction,
};

pub struct Handles<'env> {
    pub kv: Database<'env>,
    pub ipv4: Vec<(u8, Database<'env>)>,
    pub ipv6: Vec<(u8, Database<'env>)>,
}
pub fn load_ulmdb(environment: &Environment, data: &Dataset) -> Result<()> {
    let mut write = environment.begin_write()?;
    write.create_database(b"kv", DatabaseKind::Kv)?;
    write.create_database(b"ipv4", DatabaseKind::Ipv4)?;
    write.create_database(b"ipv6", DatabaseKind::Ipv6)?;
    for (key, value) in &data.kv {
        write.put(b"kv", key, value)?;
    }
    for r in &data.ipv4 {
        write.insert_prefix(
            b"ipv4",
            Prefix::new(Address::V4(r.network), r.length).unwrap(),
            &r.value.to_be_bytes(),
        )?;
    }
    for r in &data.ipv6 {
        write.insert_prefix(
            b"ipv6",
            Prefix::new(Address::V6(r.network), r.length).unwrap(),
            &r.value.to_be_bytes(),
        )?;
    }
    write.commit()?;
    Ok(())
}
pub fn load_lmdb<'env>(
    environment: &'env NativeEnvironment,
    data: &Dataset,
) -> Result<Handles<'env>> {
    let mut write = environment.begin_write()?;
    let kv = write.open_database("kv", NativeKind::Ordinary, true)?;
    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();
    for length in data
        .ipv4
        .iter()
        .map(|r| r.length)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .rev()
    {
        ipv4.push((
            length,
            write.open_database(&format!("ipv4-{length}"), NativeKind::Ordinary, true)?,
        ));
    }
    for length in data
        .ipv6
        .iter()
        .map(|r| r.length)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .rev()
    {
        ipv6.push((
            length,
            write.open_database(&format!("ipv6-{length}"), NativeKind::Ordinary, true)?,
        ));
    }
    for (key, value) in &data.kv {
        write.put(&kv, key, value)?;
    }
    for r in &data.ipv4 {
        let db = &ipv4
            .iter()
            .find(|(length, _)| *length == r.length)
            .unwrap()
            .1;
        write.put(db, &r.network.to_be_bytes(), &r.value.to_be_bytes())?;
    }
    for r in &data.ipv6 {
        let db = &ipv6
            .iter()
            .find(|(length, _)| *length == r.length)
            .unwrap()
            .1;
        write.put(db, &r.network.to_be_bytes(), &r.value.to_be_bytes())?;
    }
    write.commit()?;
    Ok(Handles { kv, ipv4, ipv6 })
}
pub struct Prefix4<'txn> {
    tables: Vec<(u8, ReadDatabase<'txn>)>,
    cover_network: u32,
    cover_length: u8,
}
pub struct Prefix6<'txn> {
    tables: Vec<(u8, ReadDatabase<'txn>)>,
    cover_network: u128,
    cover_length: u8,
}
impl<'txn> Prefix4<'txn> {
    pub fn bind<'env>(
        read: &'txn ReadTransaction<'env>,
        handles: &Handles<'env>,
        data: &Dataset,
    ) -> Result<Self> {
        let first = data.ipv4.first().ok_or("empty IPv4 fixture")?;
        let mut length = first.length;
        for r in &data.ipv4 {
            length = length
                .min(r.length)
                .min((r.network ^ first.network).leading_zeros() as u8);
        }
        let tables = handles
            .ipv4
            .iter()
            .map(|(length, db)| read.database(db).map(|view| (*length, view)))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Self {
            tables,
            cover_network: mask4(first.network, length),
            cover_length: length,
        })
    }
    pub fn get(&self, address: u32) -> ulmdb_lmdb_baseline::Result<Option<Answer>> {
        if mask4(address, self.cover_length) != self.cover_network {
            return Ok(None);
        }
        for (length, view) in &self.tables {
            if let Some(value) = view.get(&mask4(address, *length).to_be_bytes())? {
                return Ok(Some(Answer {
                    length: *length,
                    value: decode(value),
                }));
            }
        }
        Ok(None)
    }
    pub fn cover(&self) -> String {
        format!("{:08x}/{}", self.cover_network, self.cover_length)
    }
}
impl<'txn> Prefix6<'txn> {
    pub fn bind<'env>(
        read: &'txn ReadTransaction<'env>,
        handles: &Handles<'env>,
        data: &Dataset,
    ) -> Result<Self> {
        let first = data.ipv6.first().ok_or("empty IPv6 fixture")?;
        let mut length = first.length;
        for r in &data.ipv6 {
            length = length
                .min(r.length)
                .min((r.network ^ first.network).leading_zeros() as u8);
        }
        let tables = handles
            .ipv6
            .iter()
            .map(|(length, db)| read.database(db).map(|view| (*length, view)))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Self {
            tables,
            cover_network: mask6(first.network, length),
            cover_length: length,
        })
    }
    pub fn get(&self, address: u128) -> ulmdb_lmdb_baseline::Result<Option<Answer>> {
        if mask6(address, self.cover_length) != self.cover_network {
            return Ok(None);
        }
        for (length, view) in &self.tables {
            if let Some(value) = view.get(&mask6(address, *length).to_be_bytes())? {
                return Ok(Some(Answer {
                    length: *length,
                    value: decode(value),
                }));
            }
        }
        Ok(None)
    }
    pub fn cover(&self) -> String {
        format!("{:032x}/{}", self.cover_network, self.cover_length)
    }
}
pub fn decode(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().expect("eight-byte benchmark payload"))
}
