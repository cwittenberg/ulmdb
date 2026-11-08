//! Keep default pages on the stack; other persisted geometries own exact bytes.
pub(crate) enum PageBuffer {
    Default([u8; 4096]),
    Other(Vec<u8>),
}
impl PageBuffer {
    pub(crate) fn new(size: usize) -> Self {
        if size == 4096 {
            Self::Default([0; 4096])
        } else {
            Self::Other(vec![0; size])
        }
    }
}
impl std::ops::Deref for PageBuffer {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Default(b) => b,
            Self::Other(b) => b,
        }
    }
}
impl std::ops::DerefMut for PageBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Default(b) => b,
            Self::Other(b) => b,
        }
    }
}
