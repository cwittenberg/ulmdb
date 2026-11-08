//! Volatile private owner record. Linux v1 bytes remain unchanged; Darwin v2
//! stores all 64 bits returned by pthread_threadid_np, never a pthread pointer.
use crate::{Error, Result};
pub(super) const RECORD_BYTES: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Owner {
    pub pid: u32,
    pub tid: u64,
}
#[derive(Clone, Copy)]
#[allow(dead_code)] // Both wire codecs are exercised on either host.
pub(super) enum Format {
    Linux,
    Darwin,
}
#[cfg(target_os = "linux")]
pub(super) const CURRENT: Format = Format::Linux;
#[cfg(target_os = "macos")]
pub(super) const CURRENT: Format = Format::Darwin;
impl Format {
    fn fields(self) -> (&'static [u8; 8], u32, usize) {
        match self {
            Self::Linux => (b"ULMWR001", 1, 20),
            Self::Darwin => (b"ULMWR002", 2, 24),
        }
    }
}
pub(super) fn encode(owner: Owner, format: Format) -> Result<[u8; RECORD_BYTES]> {
    if owner.pid == 0 || owner.tid == 0 {
        return Err(Error::Corrupt("invalid coordinated writer owner identity"));
    }
    let (magic, version, reserved) = format.fields();
    let mut bytes = [0u8; RECORD_BYTES];
    bytes[..8].copy_from_slice(magic);
    bytes[8..12].copy_from_slice(&version.to_le_bytes());
    bytes[12..16].copy_from_slice(&owner.pid.to_le_bytes());
    if reserved == 20 {
        let tid = u32::try_from(owner.tid)
            .map_err(|_| Error::Corrupt("Linux writer thread ID overflow"))?;
        bytes[16..20].copy_from_slice(&tid.to_le_bytes());
    } else {
        bytes[16..24].copy_from_slice(&owner.tid.to_le_bytes());
    }
    let checksum = crc(&bytes[..28]);
    bytes[28..].copy_from_slice(&checksum.to_le_bytes());
    Ok(bytes)
}
pub(super) fn decode(bytes: &[u8], format: Format) -> Result<Owner> {
    let (magic, version, reserved) = format.fields();
    if bytes.len() != RECORD_BYTES
        || &bytes[..8] != magic
        || bytes[8..12] != version.to_le_bytes()
        || bytes[reserved..28].iter().any(|b| *b != 0)
        || bytes[28..] != crc(&bytes[..28]).to_le_bytes()
    {
        return Err(Error::Corrupt("invalid coordinated writer owner record"));
    }
    let pid = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let tid = if reserved == 20 {
        u64::from(u32::from_le_bytes(bytes[16..20].try_into().unwrap()))
    } else {
        u64::from_le_bytes(bytes[16..24].try_into().unwrap())
    };
    if pid == 0 || tid == 0 {
        return Err(Error::Corrupt("invalid coordinated writer owner identity"));
    }
    Ok(Owner { pid, tid })
}
fn crc(bytes: &[u8]) -> u32 {
    let mut value = !0u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(value & 1)));
        }
    }
    !value
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_owner_wire_is_v1_exact() {
        let owner = Owner {
            pid: 0x12345678,
            tid: 0x90abcdef,
        };
        let bytes = encode(owner, Format::Linux).unwrap();
        assert_eq!(&bytes[..28], b"ULMWR001\x01\x00\x00\x00\x78\x56\x34\x12\xef\xcd\xab\x90\x00\x00\x00\x00\x00\x00\x00\x00");
        assert_eq!(decode(&bytes, Format::Linux).unwrap(), owner);
        assert!(decode(&bytes, Format::Darwin).is_err());
    }
    #[test]
    fn darwin_owner_retains_high_thread_bits_and_rejects_damage() {
        let owner = Owner {
            pid: 7,
            tid: 0xfedcba9876543210,
        };
        let bytes = encode(owner, Format::Darwin).unwrap();
        assert_eq!(decode(&bytes, Format::Darwin).unwrap(), owner);
        assert!(decode(&bytes, Format::Linux).is_err());
        assert!(encode(owner, Format::Linux).is_err());
        for at in 0..RECORD_BYTES {
            let mut damaged = bytes;
            damaged[at] ^= 1;
            assert!(decode(&damaged, Format::Darwin).is_err());
        }
        assert!(encode(Owner { pid: 0, tid: 1 }, Format::Darwin).is_err());
        assert!(encode(Owner { pid: 1, tid: 0 }, Format::Darwin).is_err());
    }
}
