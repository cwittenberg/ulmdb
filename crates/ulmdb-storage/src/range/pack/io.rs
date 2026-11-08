//! Checked positional IO; no shared seek cursor and no truncate/overwrite of data.
use std::{
    fs::File,
    io::{self, ErrorKind},
};
pub(crate) trait ReadAt {
    fn read_at(&self, bytes: &mut [u8], offset: u64) -> io::Result<usize>;
}
pub(crate) trait WriteAt {
    fn write_at(&self, bytes: &[u8], offset: u64) -> io::Result<usize>;
}
#[cfg(any(unix,windows))]
impl ReadAt for File {
    fn read_at(&self, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
        ulmdb_platform::FileExt::read_at(self, bytes, offset)
    }
}
#[cfg(any(unix,windows))]
impl WriteAt for File {
    fn write_at(&self, bytes: &[u8], offset: u64) -> io::Result<usize> {
        ulmdb_platform::FileExt::write_at(self, bytes, offset)
    }
}
#[cfg(not(any(unix,windows)))]
impl ReadAt for File {
    fn read_at(&self, _: &mut [u8], _: u64) -> io::Result<usize> {
        Err(ErrorKind::Unsupported.into())
    }
}
#[cfg(not(any(unix,windows)))]
impl WriteAt for File {
    fn write_at(&self, _: &[u8], _: u64) -> io::Result<usize> {
        Err(ErrorKind::Unsupported.into())
    }
}
fn check_extent(offset: u64, length: usize) -> io::Result<()> {
    let length = u64::try_from(length).map_err(|_| io::Error::from(ErrorKind::InvalidInput))?;
    offset
        .checked_add(length)
        .ok_or_else(|| io::Error::from(ErrorKind::InvalidInput))?;
    Ok(())
}
pub(crate) fn write_all_at<W: WriteAt + ?Sized>(
    file: &W,
    mut bytes: &[u8],
    mut offset: u64,
) -> io::Result<()> {
    check_extent(offset, bytes.len())?;
    while !bytes.is_empty() {
        match file.write_at(bytes, offset) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) if n <= bytes.len() => {
                offset += n as u64;
                bytes = &bytes[n..];
            }
            Ok(_) => return Err(ErrorKind::InvalidData.into()),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
pub(crate) fn read_exact_at<R: ReadAt + ?Sized>(
    file: &R,
    mut bytes: &mut [u8],
    mut offset: u64,
) -> io::Result<()> {
    check_extent(offset, bytes.len())?;
    while !bytes.is_empty() {
        match file.read_at(bytes, offset) {
            Ok(0) => return Err(ErrorKind::UnexpectedEof.into()),
            Ok(n) if n <= bytes.len() => {
                offset += n as u64;
                bytes = &mut bytes[n..];
            }
            Ok(_) => return Err(ErrorKind::InvalidData.into()),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    struct Short {
        bytes: RefCell<Vec<u8>>,
        interrupt: Cell<bool>,
        limit: usize,
    }
    impl Short {
        fn new(limit: usize) -> Self {
            Self {
                bytes: RefCell::new(vec![0; 32]),
                interrupt: Cell::new(true),
                limit,
            }
        }
    }
    impl WriteAt for Short {
        fn write_at(&self, bytes: &[u8], offset: u64) -> io::Result<usize> {
            if self.interrupt.replace(false) {
                return Err(ErrorKind::Interrupted.into());
            }
            let n = bytes.len().min(self.limit);
            self.bytes.borrow_mut()[offset as usize..offset as usize + n]
                .copy_from_slice(&bytes[..n]);
            Ok(n)
        }
    }
    impl ReadAt for Short {
        fn read_at(&self, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
            if self.interrupt.replace(false) {
                return Err(ErrorKind::Interrupted.into());
            }
            let source = self.bytes.borrow();
            let n = bytes.len().min(self.limit);
            bytes[..n].copy_from_slice(&source[offset as usize..offset as usize + n]);
            Ok(n)
        }
    }
    #[test]
    fn all_partial_boundaries_and_interruptions_preserve_offsets_and_bytes() {
        for limit in 1..=16 {
            let file = Short::new(limit);
            write_all_at(&file, b"0123456789abcdef", 7).unwrap();
            file.interrupt.set(true);
            let mut out = [0; 16];
            read_exact_at(&file, &mut out, 7).unwrap();
            assert_eq!(&out, b"0123456789abcdef");
            assert_eq!(&file.bytes.borrow()[..7], &[0; 7]);
        }
    }
    #[test]
    fn checked_overflow_empty_and_zero_progress() {
        let file = Short::new(0);
        assert_eq!(
            write_all_at(&file, b"x", 0).unwrap_err().kind(),
            ErrorKind::WriteZero
        );
        assert_eq!(
            read_exact_at(&file, &mut [0], 0).unwrap_err().kind(),
            ErrorKind::UnexpectedEof
        );
        let file = Short::new(1);
        assert_eq!(
            write_all_at(&file, b"x", u64::MAX).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert!(file.interrupt.get());
        assert!(write_all_at(&file, b"", u64::MAX).is_ok());
        assert!(read_exact_at(&file, &mut [], u64::MAX).is_ok());
    }
}

#[cfg(test)]
mod hostile_tests {
    use super::*;
    struct Bad;
    impl WriteAt for Bad {
        fn write_at(&self, b: &[u8], _: u64) -> io::Result<usize> {
            Ok(b.len() + 1)
        }
    }
    impl ReadAt for Bad {
        fn read_at(&self, b: &mut [u8], _: u64) -> io::Result<usize> {
            Ok(b.len() + 1)
        }
    }
    #[test]
    fn overreported_progress_is_an_error_and_never_panics() {
        assert_eq!(
            write_all_at(&Bad, b"x", 0).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            read_exact_at(&Bad, &mut [0], 0).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
    }
}
