//! Canonical input and independently computed expected-answer encodings.

use super::Workload;
use ulmdb_core::Address;

impl Workload {
    /// Canonical family byte + 16 bytes (IPv4 first four, twelve zero padding).
    fn encode_address(address: Address, out: &mut Vec<u8>) {
        match address {
            Address::V4(n) => {
                out.push(4);
                out.extend_from_slice(&n.to_be_bytes());
                out.extend_from_slice(&[0; 12]);
            }
            Address::V6(n) => {
                out.push(6);
                out.extend_from_slice(&n.to_be_bytes());
            }
        }
    }

    pub fn encoded(&self) -> [(String, Vec<u8>); 4] {
        let mut p = b"ULMDBP01".to_vec();
        p.extend_from_slice(&(self.prefixes.0.len() as u64).to_le_bytes());
        for (prefix, value) in &self.prefixes.0 {
            Self::encode_address(prefix.network(), &mut p);
            p.push(prefix.length() as u8);
            p.extend_from_slice(&(value.len() as u64).to_le_bytes());
            p.extend_from_slice(value);
        }
        let mut kv = b"ULMDBK01".to_vec();
        kv.extend_from_slice(&(self.kv.len() as u64).to_le_bytes());
        for (key, value) in &self.kv {
            kv.extend_from_slice(&(key.len() as u64).to_le_bytes());
            kv.extend_from_slice(&(value.len() as u64).to_le_bytes());
            kv.extend_from_slice(key);
            kv.extend_from_slice(value);
        }
        let mut q = b"ULMDBQ01".to_vec();
        q.extend_from_slice(&(self.queries.len() as u64).to_le_bytes());
        let mut expected = b"ULMDBE01".to_vec();
        expected.extend_from_slice(&(self.queries.len() as u64).to_le_bytes());
        for &query in &self.queries {
            Self::encode_address(query, &mut q);
            if let Some((prefix, value)) = self.prefixes.longest_match(query) {
                expected.push(1);
                Self::encode_address(prefix.network(), &mut expected);
                expected.push(prefix.length() as u8);
                expected.extend_from_slice(&(value.len() as u64).to_le_bytes());
                expected.extend_from_slice(value);
            } else {
                expected.push(0);
            }
        }
        [
            ("prefixes.bin".into(), p),
            ("kv.bin".into(), kv),
            ("queries.bin".into(), q),
            ("expected.bin".into(), expected),
        ]
    }
}
