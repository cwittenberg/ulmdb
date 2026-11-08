"""Run with PYTHONPATH=clients/python python3 clients/python/examples/quickstart.py."""

from pathlib import Path
import tempfile

from ulmdb import Environment


def example(path):
    with Environment(path) as environment:
        with environment.begin(write=True) as transaction:
            users = transaction.open_db("users", create=True)
            routes4 = transaction.open_db("routes4", kind="ipv4", create=True)
            routes6 = transaction.open_db("routes6", kind="ipv6", create=True)
            transaction.put(users, b"alice", b"Alice Example")
            transaction.prefix_put(routes4, "192.0.2.0/24", b"documentation IPv4")
            transaction.prefix_put(routes6, "2001:db8::/32", b"documentation IPv6")
        with environment.begin() as transaction:
            print("User:", transaction.get(users, b"alice"))
            print("IPv4:", transaction.longest_match(routes4, "192.0.2.42"))
            print("IPv6:", transaction.longest_match(routes6, "2001:db8::42"))
            for key, value in transaction.items(users):
                print("Ordered entry:", key, value)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="ulmdb-python-example-") as temporary:
        example(Path(temporary) / "database")
