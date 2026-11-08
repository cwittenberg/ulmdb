//! Native module loading for the LMDB 1.0 crypto hook ABI.
use super::*;
#[cfg(windows)]
mod windows;
pub(super) type Encrypt =
    unsafe extern "C" fn(*const MDB_val, *mut MDB_val, *const MDB_val, c_int) -> c_int;
pub(super) type Checksum = unsafe extern "C" fn(*const MDB_val, *mut MDB_val, *const MDB_val);
pub(super) type StringToKey = unsafe extern "C" fn(*const c_char, *mut MDB_val) -> c_int;
#[repr(C)]
pub struct MDB_crypto_funcs {
    pub mcf_str2key: Option<StringToKey>,
    pub mcf_encfunc: Option<Encrypt>,
    pub mcf_sumfunc: Option<Checksum>,
    pub mcf_keysize: c_int,
    pub mcf_esumsize: c_int,
    pub mcf_sumsize: c_int,
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_modload(
    file: *const c_char,
    symbol: *const c_char,
    functions: *mut *mut MDB_crypto_funcs,
    error: *mut *mut c_char,
) -> *mut c_void {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if functions.is_null() {
            if !error.is_null() {
                unsafe { *error = c"Missing crypto hook output".as_ptr().cast_mut() };
            }
            return ptr::null_mut();
        }
        unsafe {
            *functions = ptr::null_mut();
        }
        #[cfg(unix)]
        {
            let handle = unsafe { libc::dlopen(file, libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if handle.is_null() {
                if !error.is_null() {
                    unsafe { *error = libc::dlerror() };
                }
                return ptr::null_mut();
            }
            let name = if symbol.is_null() {
                c"MDB_crypto".as_ptr()
            } else {
                symbol
            };
            let function = unsafe { libc::dlsym(handle, name) };
            if function.is_null() {
                if !error.is_null() {
                    unsafe { *error = c"Crypto hook function not found".as_ptr().cast_mut() };
                }
                unsafe {
                    libc::dlclose(handle);
                }
                return ptr::null_mut();
            }
            let hook: unsafe extern "C" fn() -> *mut MDB_crypto_funcs =
                unsafe { std::mem::transmute(function) };
            unsafe {
                *functions = hook();
            }
            handle
        }
        #[cfg(windows)]
        {
            unsafe { windows::load(file, symbol, functions, error) }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (file, symbol);
            if !error.is_null() {
                unsafe {
                    *error = c"Crypto module loader unavailable on this platform"
                        .as_ptr()
                        .cast_mut()
                };
            }
            ptr::null_mut()
        }
    }));
    result.unwrap_or(ptr::null_mut())
}
/// The caller closes every environment/cache using the hooks before unload,
/// matching LMDB's native callback lifetime contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_modunload(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    #[cfg(unix)]
    unsafe {
        libc::dlclose(handle);
    }
    #[cfg(windows)]
    unsafe { windows::unload(handle) };
    #[cfg(not(any(unix, windows)))]
    let _ = handle;
}
