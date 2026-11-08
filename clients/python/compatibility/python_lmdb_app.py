"""Public python-lmdb client fixture, unchanged between engine runs."""
import json
import lmdb
from pathlib import Path
import struct
import sys


def main():
    action, path = sys.argv[1:]
    seed = action == "seed"
    env = lmdb.open(path, map_size=64 * 1024 * 1024, max_dbs=8, max_readers=64)
    try:
        items = env.open_db(b"items", create=seed)
        duplicate = env.open_db(b"duplicate", create=seed, dupsort=True)
        integer = env.open_db(b"integer", create=seed, integerkey=True)
        fixed = env.open_db(b"fixed", create=seed, dupsort=True, dupfixed=True, integerdup=True)
        if seed:
            with env.begin(write=True, db=items) as txn:
                assert txn.put(b"hello", b"world")
                assert txn.put(b"\0binary\xff", b"\xffvalue\0")
                assert txn.put(b"empty", b"")
                assert txn.put(b"key", b"c", db=duplicate)
                assert txn.put(b"key", b"a", db=duplicate)
                assert txn.put(b"key", b"b", db=duplicate)
                for number in [256, 1, 2]:
                    assert txn.put(struct.pack("=I", number), str(number).encode(), db=integer)
                    assert txn.put(b"key", struct.pack("=I", number), db=fixed)
        else:
            with env.begin(db=items, buffers=True) as txn:
                assert bytes(txn.get(b"hello")) == b"world"
                assert bytes(txn.get(b"\0binary\xff")) == b"\xffvalue\0"
                assert bytes(txn.get(b"empty")) == b""
                assert txn.get(b"absent") is None
                with txn.cursor() as cursor:
                    assert [(bytes(k), bytes(v)) for k, v in cursor] == [
                        (b"\0binary\xff", b"\xffvalue\0"), (b"empty", b""), (b"hello", b"world")
                    ] + ([(b"updated", b"adapter")] if action == "verify-updated" else [])
                with txn.cursor(duplicate) as cursor:
                    assert cursor.set_key(b"key")
                    assert cursor.count() == 3
                    assert [bytes(value) for value in cursor.iternext_dup()] == [b"a", b"b", b"c"]
                with txn.cursor(integer) as cursor:
                    assert [struct.unpack("=I", key)[0] for key in cursor.iternext(values=False)] == [1, 2, 256]
                with txn.cursor(fixed) as cursor:
                    assert cursor.set_key(b"key")
                    assert [struct.unpack("=I", value)[0] for value in cursor.iternext_dup()] == [1, 2, 256]
            if action == "update":
                snapshot = env.begin(db=items)
                with env.begin(write=True, db=items) as txn:
                    assert txn.put(b"updated", b"adapter")
                assert snapshot.get(b"updated") is None
                snapshot.abort()
            if action == "verify-updated":
                with env.begin(db=items) as txn:
                    assert txn.get(b"updated") == b"adapter"
        env.sync(True)
        assert env.max_key_size() >= 511
        assert env.flags()["readonly"] is False
        assert env.info()["max_readers"] == 64
        print(json.dumps({"python_lmdb": lmdb.__version__, "lmdb_api": lmdb.version(), "action": action}))
    finally:
        env.close()


if __name__ == "__main__":
    main()
