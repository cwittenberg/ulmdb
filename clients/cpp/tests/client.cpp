#include "support.hpp"
#include <atomic>
#include <thread>
#include <vector>

static void test_binary_snapshots_and_cursors() {
    fixture files;
    ulmdb::environment env(files.path("data"));
    ulmdb::database kv, duplicates;
    const std::string binary_key("k\0\xff", 3), binary_value("v\0\xff\x80", 4);
    {
        auto write = env.begin_write();
        kv = write.open_database("items", ulmdb::database_kind::kv, true);
        duplicates = write.open_database("dups", ulmdb::database_kind::duplicates, true);
        require(write.put(kv, binary_key, binary_value), "put binary record");
        require(write.put(kv, "empty", ""), "put empty value");
        require(!write.put(kv, binary_key, "different", {false, false}), "no overwrite");
        write.put(duplicates, "group", "c"); write.put(duplicates, "group", "a");
        write.put(duplicates, "group", "b");
        require(!write.put(duplicates, "group", "b", {true, true}), "no duplicate data");
        write.commit();
    }
    auto snapshot = env.begin_read();
    auto owned_value = snapshot.get(kv, binary_key);
    require(owned_value == binary_value, "binary value preserves embedded NUL");
    require(snapshot.get(kv, "empty") == std::string{}, "empty value is present");
    require(!snapshot.get(kv, "absent"), "absent value");
    {
        auto write = env.begin_write();
        write.put(kv, "later", "committed");
        auto cursor = write.open_cursor(duplicates);
        require(cursor.seek_exact("group")->second == "a", "first duplicate");
        require(cursor.duplicate_count() == 3, "duplicate count");
        require(cursor.next_duplicate()->second == "b", "next duplicate");
        cursor.erase();
        require(cursor.current()->second == "c", "cursor current after deletion");
        cursor.replace("d");
        require(cursor.last_duplicate()->second == "d", "replace duplicate value");
        write.commit();
        expect_error(MDB_BAD_TXN, [&] { cursor.next(); });
    }
    require(!snapshot.get(kv, "later"), "old snapshot excludes later publication");
    auto old_cursor = snapshot.open_cursor(duplicates);
    std::vector<std::string> old_duplicates;
    for (const auto& item : old_cursor) old_duplicates.push_back(item.second);
    require(old_duplicates == std::vector<std::string>({"a", "b", "c"}), "snapshot cursor isolation");
    snapshot.abort();
    expect_error(MDB_BAD_TXN, [&] { old_cursor.first(); });
    require(owned_value == binary_value, "owned values survive transaction close");
    auto fresh = env.begin_read();
    require(fresh.get(kv, "later") == "committed", "new snapshot sees commit");
    require(fresh.size(duplicates) == 2, "logical duplicate count");
    auto cursor = fresh.open_cursor(duplicates);
    std::vector<ulmdb::record> items(cursor.begin(), cursor.end());
    require(items == std::vector<ulmdb::record>({{"group", "a"}, {"group", "d"}}), "input iterator ordered records");
    require(cursor.seek_both("group", "b", true)->second == "d", "duplicate range seek");
    expect_error(EACCES, [&] { fresh.put(kv, "readonly", "rejected"); });
    expect_error(EACCES, [&] { cursor.erase(); });
    fresh.abort();
    std::cout << "PASS: binary KV, duplicate cursors, owned results and snapshots\n";
}
static void test_moves_readonly_and_threads() {
    fixture files;
    ulmdb::environment original(files.path("data"));
    ulmdb::environment env(std::move(original));
    expect_error(EINVAL, [&] { original.begin_read(); });
    ulmdb::database db;
    {
        auto write = env.begin_write();
        db = write.open_database("db", ulmdb::database_kind::kv, true);
        write.put(db, "key", "value"); write.commit();
    }
    auto initial = env.begin_read();
    auto read = std::move(initial);
    expect_error(MDB_BAD_TXN, [&] { initial.get(db, "key"); });
    auto first = read.open_cursor(db);
    auto cursor = std::move(first);
    expect_error(MDB_BAD_TXN, [&] { first.first(); });
    require(cursor.first()->second == "value", "moved cursor");
    std::atomic<bool> rejected{false};
    std::thread worker([&] {
        try { (void)read.get(db, "key"); }
        catch (const ulmdb::error& error) { rejected.store(error.code() == MDB_BAD_TXN); }
    });
    worker.join();
    require(rejected.load(), "wrong-thread transaction access rejected");
    read.abort();
    ulmdb::environment_options options; options.readonly = true;
    ulmdb::environment readonly(files.path("data"), options);
    expect_error(EACCES, [&] { readonly.begin_write(); });
    auto reader = readonly.begin_read();
    auto own_db = reader.open_database("db");
    require(reader.get(own_db, "key") == "value", "read-only reopen");
    expect_error(EINVAL, [&] { reader.open_database(std::string("bad\0name", 8)); });
    std::cout << "PASS: move semantics, read-only environments and thread guards\n";
}
int main() {
    try {
        test_binary_snapshots_and_cursors();
        test_moves_readonly_and_threads();
        test_lifetimes();
        test_routes_and_c_interoperability();
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n'; return 1;
    }
}
