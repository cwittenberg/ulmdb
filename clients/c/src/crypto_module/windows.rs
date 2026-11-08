//! Win32 module ownership. UTF-8 C paths become UTF-16; hook symbols retain
//! their native byte names. Error storage follows the thread-local dlerror model.
use super::MDB_crypto_funcs;
use std::{
    cell::RefCell,
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(filename: *const u16) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, name: *const u16, output: *mut *mut c_void) -> i32;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}
thread_local! {
    static ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}
fn message(output: *mut *mut c_char, operation: &str, code: u32) {
    if output.is_null() {
        return;
    }
    ERROR.with(|slot| {
        let text = CString::new(format!("{operation}: Win32 error {code}"))
            .expect("static loader error contains no NUL");
        let mut slot = slot.borrow_mut();
        *slot = Some(text);
        // SAFETY: the C caller supplied this output. Its string remains owned
        // by this thread until the next loader failure or thread termination.
        unsafe { *output = slot.as_ref().unwrap().as_ptr().cast_mut() };
    });
}
pub(super) unsafe fn load(
    file: *const c_char,
    symbol: *const c_char,
    functions: *mut *mut MDB_crypto_funcs,
    error: *mut *mut c_char,
) -> *mut c_void {
    let handle = if file.is_null() {
        let mut module = ptr::null_mut();
        // Increment the executable's module reference, so unload owns exactly
        // the same release obligation as a successful LoadLibraryW call.
        if unsafe { GetModuleHandleExW(0, ptr::null(), &mut module) } == 0 {
            message(error, "GetModuleHandleExW", unsafe { GetLastError() });
            return ptr::null_mut();
        }
        module
    } else {
        // SAFETY: the public FFI caller guarantees a live terminated C string.
        let text = match unsafe { CStr::from_ptr(file) }.to_str() {
            Ok(text) => text,
            Err(_) => {
                message(error, "Invalid UTF-8 crypto module path", 1113);
                return ptr::null_mut();
            }
        };
        let path = text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let module = unsafe { LoadLibraryW(path.as_ptr()) };
        if module.is_null() {
            message(error, "LoadLibraryW", unsafe { GetLastError() });
            return ptr::null_mut();
        }
        module
    };
    let name = if symbol.is_null() {
        c"MDB_crypto".as_ptr()
    } else {
        symbol
    };
    let address = unsafe { GetProcAddress(handle, name) };
    if address.is_null() {
        let code = unsafe { GetLastError() };
        unsafe { FreeLibrary(handle) };
        message(error, "Crypto hook function not found", code);
        return ptr::null_mut();
    }
    // SAFETY: the trusted native module exports the documented C hook ABI.
    let hook: unsafe extern "C" fn() -> *mut MDB_crypto_funcs =
        unsafe { std::mem::transmute(address) };
    unsafe { *functions = hook() };
    handle
}
pub(super) unsafe fn unload(handle: *mut c_void) {
    if !handle.is_null() {
        unsafe { FreeLibrary(handle) };
    }
}
