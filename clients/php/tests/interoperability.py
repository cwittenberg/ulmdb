#!/usr/bin/env python3
"""Python -> PHP -> Python exchange, executed only inside the PHP test image."""

from pathlib import Path
import subprocess
import tempfile

from ulmdb import Environment


def main():
    with tempfile.TemporaryDirectory(prefix="ulmdb-php-interop-") as temporary:
        path = Path(temporary) / "database"
        with Environment(path) as environment:
            with environment.begin(write=True) as transaction:
                kv = transaction.open_db("interop-kv", create=True)
                duplicates = transaction.open_db("interop-duplicates", kind="duplicates", create=True)
                v4 = transaction.open_db("interop-v4", kind="ipv4", create=True)
                v6 = transaction.open_db("interop-v6", kind="ipv6", create=True)
                transaction.put(kv, b"\0python\xff", b"python\0value\xff")
                transaction.put(kv, b"empty", b"")
                transaction.put(duplicates, b"key", b"z")
                transaction.put(duplicates, b"key", b"a")
                transaction.prefix_put(v4, "192.0.2.0/24", b"python-v4")
                transaction.prefix_put(v6, "2001:db8::/32", b"python-v6")
        subprocess.run(["php", str(Path(__file__).with_suffix(".php")), str(path)], check=True)
        with Environment(path, readonly=True) as environment:
            with environment.begin() as transaction:
                kv = transaction.open_db("interop-kv")
                duplicates = transaction.open_db("interop-duplicates", kind="duplicates")
                v4 = transaction.open_db("interop-v4", kind="ipv4")
                v6 = transaction.open_db("interop-v6", kind="ipv6")
                assert transaction.get(kv, b"\0python\xff") == b"python\0value\xff"
                assert transaction.get(kv, b"\0php\xfe") == b"php\0value\xfe"
                assert transaction.get(kv, b"empty") == b""
                assert list(transaction.items(duplicates)) == [(b"key", b"a"), (b"key", b"m"), (b"key", b"z")]
                assert transaction.longest_match(v4, "192.0.2.200") == ("192.0.2.128/25", b"php-v4")
                assert transaction.longest_match(v6, "2001:db8:1::1") == ("2001:db8:1::/48", b"php-v6")
        print("PASS: Python -> PHP -> Python persisted binary KV, empty values, duplicates, IPv4 and IPv6")


if __name__ == "__main__":
    main()
