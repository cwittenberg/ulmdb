//! Physical coordination for Linux, macOS and Windows for persisted immutable roots, independent of LMDB slots.
//!
//! Capture uses a shared gate; maintenance uses it exclusively. Registration and
//! scans also take the registry gate exclusively, always after the capture gate.
//! A physical pin verifies the persisted descriptor and its exact checkpoint
//! binding, not the logical object graph. Callers must perform graph/residency
//! validation while still holding capture and retain their existing writer fence
//! when publishing. The module does not select or recover an engine backend.
//!
//! Public capture/maintenance acquisition is nonblocking: a conflicting
//! same-process capability returns `Busy` instead of waiting for itself. Only
//! the private registry serializes concurrent registrations with a blocking
//! lease; no holder can wait for another gate or escape under shared capture.
//! Coordination metadata requires a
//! writable directory. Existing pure read-only snapshot APIs remain separate.
//! Direct external replacement/modification of immutable files or lease paths is
//! outside the cooperating protocol. Inherited capabilities cannot be used after
//! fork; a foreign destructor cannot unlock or unlink the creator's lease.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod linux;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use linux::*;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub use unsupported::*;

mod writer;
pub use writer::WriterLease;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(windows)]
pub type DirectoryIdentity = (u64, u128);
#[cfg(not(windows))]
pub type DirectoryIdentity = (u64, u64);
