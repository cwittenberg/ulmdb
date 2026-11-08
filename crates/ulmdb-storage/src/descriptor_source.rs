//! One-frame authenticated descriptor source. This grants no namespace or pin
//! authority; its caller keeps the immutable source protected across steps.
use crate::{
    Error, Result,
    creator_owned::CreatorOwned,
    transform::{self, DecodedLease, ObjectKind, TransformContext},
};
use std::{fs::File, sync::Arc};
use ulmdb_platform::{FileExt, FileIdentity};

const HEADER: usize = 128;
const PAGE: usize = 4096;

#[derive(Default)]
pub(crate) struct ReadProgress {
    pub copied: usize,
    pub read_bytes: usize,
    pub work_units: usize,
    pub required_read_bytes: usize,
}
struct Decoder {
    context: Arc<TransformContext>,
    id: u128,
    header: Option<[u8; HEADER]>,
    scratch: transform::FrameScratch,
    bytes: Vec<u8>,
    page: Option<u64>,
    // Own the decoded page until after its buffer and callbacks are dropped.
    _lease: DecodedLease,
}
pub(crate) struct DescriptorSource {
    creator: u32,
    file: File,
    identity: FileIdentity,
    base: u64,
    length: usize,
    decoder: Option<CreatorOwned<Decoder>>,
}
impl DescriptorSource {
    pub(crate) fn new(
        file: File,
        base: u64,
        length: usize,
        transformed: Option<(u128, Arc<TransformContext>)>,
    ) -> Result<Self> {
        let identity = ulmdb_platform::file_identity(&file)?;
        let encoded = transformed
            .as_ref()
            .map(|(_, c)| c.encoded_length(length as u64))
            .transpose()?
            .unwrap_or(length as u64);
        if !identity.is_file()
            || base
                .checked_add(encoded)
                .is_none_or(|end| end > identity.length)
        {
            return Err(Error::Corrupt("descriptor source extent"));
        }
        let decoder = transformed
            .map(|(id, context)| {
                let lease = context.reserve_cache_mapping(PAGE as u64)?;
                let scratch = transform::FrameScratch::new(&context);
                Ok::<_, Error>(CreatorOwned::new(Decoder {
                    context,
                    id,
                    header: None,
                    scratch,
                    bytes: vec![0; PAGE],
                    page: None,
                    _lease: lease,
                }))
            })
            .transpose()?;
        Ok(Self {
            creator: std::process::id(),
            file,
            identity,
            base,
            length,
            decoder,
        })
    }
    pub(crate) fn is_transformed(&self) -> bool {
        self.decoder.is_some()
    }
    pub(crate) fn check(&self) -> Result<()> {
        check(self.creator, &self.file, self.identity)
    }
    /// A frame is an atomic, bounded decode unit (<=4640 encoded bytes). When
    /// the caller supplies less, return its exact required byte budget without
    /// reading or invoking user callbacks. Cache hits copy only the requested
    /// logical bytes; every physical read and frame decode is reported.
    pub(crate) fn read(
        &mut self,
        at: usize,
        into: &mut [u8],
        bytes: usize,
        units: usize,
    ) -> Result<ReadProgress> {
        self.check()?;
        if at
            .checked_add(into.len())
            .is_none_or(|end| end > self.length)
        {
            return Err(Error::Corrupt("descriptor logical read extent"));
        }
        if into.is_empty() {
            return Ok(ReadProgress::default());
        }
        let Some(decoder) = &mut self.decoder else {
            let take = into.len().min(bytes);
            if take == 0 {
                return Ok(ReadProgress::default());
            }
            self.file.read_exact_at(
                &mut into[..take],
                self.base
                    .checked_add(at as u64)
                    .ok_or(Error::LimitExceeded)?,
            )?;
            check(self.creator, &self.file, self.identity)?;
            return Ok(ReadProgress {
                copied: take,
                read_bytes: take,
                ..Default::default()
            });
        };
        let decoder: &mut Decoder = &mut *decoder;
        if decoder.header.is_none() {
            if units == 0 {
                return Ok(ReadProgress::default());
            }
            if bytes < HEADER {
                return Ok(ReadProgress {
                    required_read_bytes: HEADER,
                    ..Default::default()
                });
            }
            decoder.header = Some(decoder.context.checked_object_header(
                &self.file,
                self.base,
                ObjectKind::Descriptor,
                decoder.id,
                self.length as u64,
            )?);
            check(self.creator, &self.file, self.identity)?;
            return Ok(ReadProgress {
                read_bytes: HEADER,
                work_units: 1,
                ..Default::default()
            });
        }
        let page = at as u64 / PAGE as u64;
        if decoder.page != Some(page) {
            if units == 0 {
                return Ok(ReadProgress::default());
            }
            let frame = usize::try_from(decoder.context.encoded_length(1)? - HEADER as u64)
                .map_err(|_| Error::LimitExceeded)?;
            if bytes < frame {
                return Ok(ReadProgress {
                    required_read_bytes: frame,
                    ..Default::default()
                });
            }
            decoder.page = None;
            let count = (self.length as u64 - page * PAGE as u64).min(PAGE as u64) as usize;
            let result = transform::decode_frame(
                &decoder.context,
                &self.file,
                self.base,
                decoder.header.as_ref().unwrap(),
                page,
                &mut decoder.bytes[..count],
                &mut decoder.scratch,
            );
            // A callback can fork or replace its source. Reject that before any
            // metadata query, cache publication, or further callback use.
            check(self.creator, &self.file, self.identity)?;
            result?;
            decoder.page = Some(page);
            return Ok(ReadProgress {
                read_bytes: frame,
                work_units: 1,
                ..Default::default()
            });
        }
        let offset = at % PAGE;
        let take = into.len().min(PAGE - offset).min(self.length - at);
        into[..take].copy_from_slice(&decoder.bytes[offset..offset + take]);
        Ok(ReadProgress {
            copied: take,
            ..Default::default()
        })
    }
}
fn check(creator: u32, file: &File, identity: FileIdentity) -> Result<()> {
    if creator != std::process::id() {
        return Err(Error::InvalidInput("inherited descriptor source"));
    }
    if ulmdb_platform::file_identity(file)? != identity {
        return Err(Error::Corrupt("descriptor source changed"));
    }
    Ok(())
}
