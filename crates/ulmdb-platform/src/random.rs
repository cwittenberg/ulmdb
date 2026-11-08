//! CNG's system-preferred RNG, with no process-global algorithm handle.
use std::io;

// Windows ULONG bounds each native request. Injection exercises short chunks
// and failure handling on non-Windows test hosts without claiming CNG execution.
fn fill_with(
    bytes: &mut [u8],
    chunk_bytes: u32,
    mut generate: impl FnMut(&mut [u8]) -> i32,
    mut error: impl FnMut(i32) -> io::Error,
) -> io::Result<()> {
    assert!(chunk_bytes != 0);
    let mut failure = None;
    for chunk in bytes.chunks_mut(chunk_bytes as usize) {
        let status = generate(chunk);
        // NT_SUCCESS accepts nonnegative NTSTATUS, including informational codes.
        if status < 0 {
            failure = Some(status);
            break;
        }
    }
    if let Some(status) = failure {
        bytes.fill(0);
        Err(error(status))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
#[link(name = "bcrypt")]
#[allow(unsafe_code)]
unsafe extern "system" {
    fn BCryptGenRandom(
        algorithm: *mut std::ffi::c_void,
        buffer: *mut u8,
        length: u32,
        flags: u32,
    ) -> i32;
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub(super) fn fill(bytes: &mut [u8]) -> io::Result<()> {
    const SYSTEM_PREFERRED: u32 = 0x00000002;
    fill_with(
        bytes,
        u32::MAX,
        |chunk| {
            // SAFETY: the uniquely borrowed writable slice covers length bytes;
            // CNG retains no pointer. System-preferred mode requires NULL handle.
            unsafe {
                BCryptGenRandom(
                    std::ptr::null_mut(),
                    chunk.as_mut_ptr(),
                    u32::try_from(chunk.len()).expect("bounded CNG request"),
                    SYSTEM_PREFERRED,
                )
            }
        },
        |status| {
            // SAFETY: this existing platform binding consumes one scalar status
            // and returns a Win32 error, not GetLastError's unrelated TLS value.
            let code = unsafe { crate::windows::RtlNtStatusToDosError(status) };
            if code == 0 {
                io::Error::other(format!("BCryptGenRandom failed: NTSTATUS {status:#x}"))
            } else {
                io::Error::from_raw_os_error(code as i32)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn injected_success_fills_every_bounded_chunk() {
        let mut bytes = [0; 8];
        let mut calls = 0;
        fill_with(
            &mut bytes,
            3,
            |chunk| {
                calls += 1;
                assert!(chunk.len() <= 3);
                chunk.fill(calls);
                0
            },
            |_| panic!("success has no error conversion"),
        )
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(bytes, [1, 1, 1, 2, 2, 2, 3, 3]);
    }
    #[test]
    fn injected_failure_clears_all_output_and_preserves_error() {
        let mut bytes = [0xa7; 8];
        let mut calls = 0;
        let status = 0xc000000du32 as i32;
        let result = fill_with(
            &mut bytes,
            3,
            |chunk| {
                calls += 1;
                chunk.fill(0x61);
                if calls == 2 { status } else { 0 }
            },
            |seen| {
                assert_eq!(seen, status);
                io::Error::from_raw_os_error(87)
            },
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(87));
        assert_eq!(calls, 2);
        assert_eq!(bytes, [0; 8]);
    }
    #[test]
    fn informational_success_and_empty_requests_follow_ntstatus_contract() {
        let mut bytes = [0; 1];
        fill_with(
            &mut bytes,
            1,
            |chunk| {
                chunk[0] = 42;
                1
            },
            |_| unreachable!(),
        )
        .unwrap();
        assert_eq!(bytes, [42]);
        fill_with(
            &mut [],
            1,
            |_| panic!("empty request has no OS call"),
            |_| unreachable!(),
        )
        .unwrap();
    }
}
