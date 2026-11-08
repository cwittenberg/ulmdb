import gc
from pathlib import Path
import tempfile
import threading
import unittest

from ulmdb import Environment, Error


class ClientTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ulmdb-python-")
        self.path = Path(self.temporary.name) / "database"
        self.environment = Environment(self.path)

    def tearDown(self):
        gc.collect()
        self.environment.close()
        self.temporary.cleanup()

    def test_binary_values_empty_values_and_persistence(self):
        with self.environment.begin(write=True) as transaction:
            db = transaction.open_db("bytes", create=True)
            transaction.put(db, b"a\0b", b"\0\xffvalue\0")
            transaction.put(db, b"empty", b"")
            self.assertEqual(transaction.get(db, b"a\0b"), b"\0\xffvalue\0")
        self.environment.close()
        self.environment = Environment(self.path)
        with self.environment.begin() as transaction:
            db = transaction.open_db("bytes")
            self.assertEqual(transaction.get(db, b"a\0b"), b"\0\xffvalue\0")
            self.assertEqual(transaction.get(db, b"empty"), b"")
            self.assertIsNone(transaction.get(db, b"missing"))

    def test_unnamed_database(self):
        with self.environment.begin(write=True) as transaction:
            db = transaction.open_db()
            transaction.put(db, b"key", b"value")
        with self.environment.begin() as transaction:
            self.assertEqual(transaction.get(transaction.open_db(), b"key"), b"value")

    def test_abort_and_exception_context(self):
        with self.environment.begin(write=True) as transaction:
            db = transaction.open_db("kv", create=True)
            transaction.put(db, b"key", b"original")
        transaction = self.environment.begin(write=True)
        transaction.put(db, b"key", b"aborted")
        transaction.abort()
        transaction.abort()
        with self.assertRaisesRegex(ValueError, "rollback"):
            with self.environment.begin(write=True) as transaction:
                transaction.put(db, b"key", b"exception")
                raise ValueError("rollback")
        with self.environment.begin() as transaction:
            self.assertEqual(transaction.get(db, b"key"), b"original")

    def test_aborted_database_creation(self):
        transaction = self.environment.begin(write=True)
        transaction.open_db("aborted", create=True)
        transaction.abort()
        with self.environment.begin() as transaction:
            with self.assertRaises(Error) as caught:
                transaction.open_db("aborted")
            self.assertEqual(caught.exception.code, -30798)

    def test_snapshot_and_copied_values(self):
        with self.environment.begin(write=True) as transaction:
            db = transaction.open_db("kv", create=True)
            transaction.put(db, b"key", b"before")
        with self.environment.begin() as before:
            copied = before.get(db, b"key")
            with self.environment.begin(write=True) as writer:
                writer.put(db, b"key", b"after")
            self.assertEqual(before.get(db, b"key"), b"before")
            with self.environment.begin() as after:
                self.assertEqual(after.get(db, b"key"), b"after")
        self.assertEqual(copied, b"before")

    def test_sorted_cursor_and_cursor_lifetime(self):
        with self.environment.begin(write=True) as transaction:
            db = transaction.open_db("kv", create=True)
            for key in (b"z", b"a", b"m"):
                transaction.put(db, key, key.upper())
        with self.environment.begin() as transaction:
            self.assertEqual(list(transaction.items(db)),
                             [(b"a", b"A"), (b"m", b"M"), (b"z", b"Z")])
            with transaction.items(db) as cursor:
                self.assertEqual(next(cursor), (b"a", b"A"))
            self.assertEqual(list(cursor), [])
            cursor = transaction.items(db)
            self.assertEqual(next(cursor), (b"a", b"A"))
        self.assertEqual(list(cursor), [])

    def test_overwrite_error_and_delete_presence(self):
        with self.environment.begin(write=True) as transaction:
            db = transaction.open_db("kv", create=True)
            transaction.put(db, b"key", b"value", overwrite=False)
            with self.assertRaises(Error) as caught:
                transaction.put(db, b"key", b"other", overwrite=False)
            self.assertEqual(caught.exception.code, -30799)
            self.assertEqual(transaction.get(db, b"key"), b"value")
            self.assertTrue(transaction.delete(db, b"key"))
            self.assertFalse(transaction.delete(db, b"key"))

    def test_sorted_duplicates(self):
        with self.environment.begin(write=True) as transaction:
            db = transaction.open_db("dups", kind="duplicates", create=True)
            for value in (b"z", b"a", b"m", b"a"):
                transaction.put(db, b"key", value)
            self.assertEqual(transaction.get(db, b"key"), b"a")
            self.assertEqual(list(transaction.items(db)),
                             [(b"key", b"a"), (b"key", b"m"), (b"key", b"z")])
            with self.assertRaises(Error) as caught:
                transaction.put(db, b"key", b"a", no_dup_data=True)
            self.assertEqual(caught.exception.code, -30799)
            self.assertTrue(transaction.delete(db, b"key", b"m"))
            self.assertFalse(transaction.delete(db, b"key", b"missing"))
            self.assertTrue(transaction.delete(db, b"key"))
            self.assertIsNone(transaction.get(db, b"key"))

    def test_ipv4_and_ipv6_native_indexes(self):
        with self.environment.begin(write=True) as transaction:
            v4 = transaction.open_db("routing4", kind="ipv4", create=True)
            v6 = transaction.open_db("routing6", kind="ipv6", create=True)
            transaction.prefix_put(v4, "10.0.0.0/8", b"private")
            transaction.prefix_put(v4, "10.2.0.0/16", b"specific")
            transaction.prefix_put(v6, "2001:db8::/32", b"documentation")
            transaction.prefix_put(v6, "2001:db8:1234::/48", b"specific6")
        with self.environment.begin() as transaction:
            self.assertEqual(transaction.longest_match(v4, "10.2.3.4"),
                             ("10.2.0.0/16", b"specific"))
            self.assertEqual(transaction.longest_match(v4, "10.3.3.4"),
                             ("10.0.0.0/8", b"private"))
            self.assertEqual(transaction.longest_match(v6, "2001:db8:1234::1"),
                             ("2001:db8:1234::/48", b"specific6"))
            self.assertEqual(transaction.prefix_get(v6, "2001:db8::/32"), b"documentation")
            self.assertIsNone(transaction.prefix_get(v4, "10.3.0.0/16"))
            self.assertIsNone(transaction.longest_match(v4, "192.0.2.1"))
            with self.assertRaises(Error):
                transaction.longest_match(v4, "2001:db8::1")
            with self.assertRaises(Error):
                transaction.longest_match(v6, "not an IP address")
        with self.environment.begin(write=True) as transaction:
            self.assertTrue(transaction.prefix_delete(v4, "10.2.0.0/16"))
            self.assertFalse(transaction.prefix_delete(v4, "10.2.0.0/16"))
            self.assertEqual(transaction.longest_match(v4, "10.2.3.4"),
                             ("10.0.0.0/8", b"private"))

    def test_read_only_transactions_and_environment(self):
        with self.environment.begin(write=True) as transaction:
            db = transaction.open_db("kv", create=True)
            transaction.put(db, b"key", b"value")
        with self.environment.begin() as transaction:
            with self.assertRaisesRegex(RuntimeError, "read-only"):
                transaction.put(db, b"key", b"other")
            with self.assertRaisesRegex(RuntimeError, "read-only"):
                transaction.open_db("other", create=True)
        self.environment.close()
        self.environment = Environment(self.path, readonly=True)
        with self.environment.begin() as transaction:
            self.assertEqual(transaction.get(transaction.open_db("kv"), b"key"), b"value")
        with self.assertRaisesRegex(RuntimeError, "read-only"):
            self.environment.begin(write=True)

    def test_closed_handles_and_active_transaction_guard(self):
        transaction = self.environment.begin(write=True)
        db = transaction.open_db("kv", create=True)
        with self.assertRaisesRegex(RuntimeError, "active transactions"):
            self.environment.close()
        with self.assertRaisesRegex(RuntimeError, "active writer"):
            self.environment.begin(write=True)
        transaction.abort()
        with self.assertRaisesRegex(RuntimeError, "closed"):
            transaction.get(db, b"key")
        self.environment.close()
        self.environment.close()
        with self.assertRaisesRegex(RuntimeError, "closed"):
            self.environment.begin()

    def test_foreign_database_and_invalid_inputs(self):
        with Environment(Path(self.temporary.name) / "other") as other:
            with other.begin(write=True) as transaction:
                foreign_db = transaction.open_db("other", create=True)
            with self.environment.begin(write=True) as transaction:
                db = transaction.open_db("kv", create=True)
                with self.assertRaisesRegex(ValueError, "different environment"):
                    transaction.get(foreign_db, b"key")
                with self.assertRaises(TypeError):
                    transaction.put(db, "text key", b"value")
                with self.assertRaises(ValueError):
                    transaction.open_db("bad\0name", create=True)
                with self.assertRaises(ValueError):
                    transaction.open_db("bad", kind="unknown")

    def test_transaction_thread_guard(self):
        transaction = self.environment.begin()
        errors = []

        def cross_thread():
            try:
                transaction.open_db()
            except RuntimeError as error:
                errors.append(str(error))

        worker = threading.Thread(target=cross_thread)
        worker.start()
        worker.join()
        transaction.abort()
        self.assertEqual(errors, ["transaction must be used on its creating thread"])


if __name__ == "__main__":
    unittest.main()
