"""The C ABI declarations. There are no Python extension dependencies."""

import ctypes as C
import ctypes.util
import os
from pathlib import Path


class Value(C.Structure):
    _fields_ = [("size", C.c_size_t), ("data", C.c_void_p)]


def load_library(path=None):
    explicit = path or os.environ.get("ULMDB_LIBRARY")
    if explicit:
        candidates = [os.fspath(explicit)]
    else:
        root = Path(__file__).resolve().parents[3]
        names = ("libulmdb_c.so", "libulmdb_c.dylib", "ulmdb_c.dll")
        candidates = [str(root / "target" / profile / name)
                      for profile in ("debug", "release") for name in names]
        installed = ctypes.util.find_library("ulmdb_c")
        if installed:
            candidates.append(installed)
    failures = []
    for candidate in candidates:
        try:
            lib = C.CDLL(candidate)
            break
        except OSError as error:
            failures.append(str(error))
    else:
        raise OSError(
            "Cannot load ULMDB. Build `cargo build -p ulmdb-c` from the repository "
            "root, or set ULMDB_LIBRARY to the shared library path.\n" + "\n".join(failures)
        )
    handle = C.c_void_p
    uint = C.c_uint
    value_ptr = C.POINTER(Value)
    signatures = {
        "mdb_env_create": ([C.POINTER(handle)], C.c_int),
        "mdb_env_set_maxdbs": ([handle, uint], C.c_int),
        "mdb_env_open": ([handle, C.c_char_p, uint, uint], C.c_int),
        "mdb_env_close": ([handle], None),
        "mdb_txn_begin": ([handle, handle, uint, C.POINTER(handle)], C.c_int),
        "mdb_txn_commit": ([handle], C.c_int),
        "mdb_txn_abort": ([handle], None),
        "ulmdb_dbi_open": ([handle, C.c_char_p, uint, uint, C.POINTER(uint)], C.c_int),
        "mdb_get": ([handle, uint, value_ptr, value_ptr], C.c_int),
        "mdb_put": ([handle, uint, value_ptr, value_ptr, uint], C.c_int),
        "mdb_del": ([handle, uint, value_ptr, value_ptr], C.c_int),
        "mdb_cursor_open": ([handle, uint, C.POINTER(handle)], C.c_int),
        "mdb_cursor_get": ([handle, value_ptr, value_ptr, C.c_int], C.c_int),
        "mdb_cursor_close": ([handle], None),
        "mdb_strerror": ([C.c_int], C.c_char_p),
        "ulmdb_prefix_put": ([handle, uint, C.c_char_p, value_ptr], C.c_int),
        "ulmdb_prefix_get": ([handle, uint, C.c_char_p, value_ptr], C.c_int),
        "ulmdb_prefix_delete": ([handle, uint, C.c_char_p], C.c_int),
        "ulmdb_longest_match": ([handle, uint, C.c_char_p, value_ptr, value_ptr], C.c_int),
    }
    for name, (arguments, result) in signatures.items():
        function = getattr(lib, name)
        function.argtypes = arguments
        function.restype = result
    return lib


def input_value(data):
    if isinstance(data, str):
        raise TypeError("keys and values must be bytes or a buffer, not str")
    try:
        raw = memoryview(data).tobytes()
    except TypeError:
        raise TypeError("keys and values must be bytes or a buffer") from None
    backing = C.create_string_buffer(raw)
    return Value(len(raw), C.cast(backing, C.c_void_p)), backing


def output_value(value):
    return C.string_at(value.data, value.size)


def text(value):
    raw = value.encode("utf-8") if isinstance(value, str) else bytes(value)
    if b"\0" in raw:
        raise ValueError("embedded NUL is not allowed in database names or addresses")
    return raw
