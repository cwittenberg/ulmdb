#include "support.hpp"

void test_lifetimes() {
    fixture files;
    ulmdb::environment env(files.path("data"));
    ulmdb::database db, rolled_back, reopened_provisional;
    {
        auto root = env.begin_write();
        db = root.open_database("durable", ulmdb::database_kind::kv, true);
        root.put(db, "initial", "value"); root.commit();
    }
    {
        auto root = env.begin_write();
        root.put(db, "parent", "private");
        {
            auto child = root.nested();
            rolled_back = child.open_database("provisional", ulmdb::database_kind::kv, true);
            reopened_provisional = child.open_database("provisional");
            child.put(rolled_back, "key", "child");
            expect_error(MDB_BAD_TXN, [&] { root.get(db, "initial"); });
            child.commit();
        }
        require(root.get(rolled_back, "key") == "child", "child commit visible to parent");
        // Outer rollback must invalidate every view of a child-created database.
    }
    require(!rolled_back.valid() && !reopened_provisional.valid(), "outer abort invalidates provisional controls");
    {
        auto root = env.begin_write();
        auto reused = root.open_database("new-slot", ulmdb::database_kind::kv, true);
        root.put(reused, "key", "replacement");
        expect_error(MDB_BAD_DBI, [&] { root.get(rolled_back, "key"); });
        root.commit();
    }
    {
        auto root = env.begin_write();
        {
            auto child = root.nested(); child.put(db, "aborted-child", "invisible");
        }
        require(!root.get(db, "aborted-child"), "nested destructor rolls back");
        root.put(db, "committed-parent", "visible"); root.commit();
    }
    auto snapshot = env.begin_read();
    require(!snapshot.get(db, "parent"), "outer destructor rolls back");
    require(snapshot.get(db, "committed-parent") == "visible", "parent continues after child abort");
    ulmdb::environment other(files.path("other"));
    auto wrong_environment = other.begin_read();
    expect_error(MDB_BAD_DBI, [&] { wrong_environment.get(db, "initial"); });
    snapshot.abort(); wrong_environment.abort();
    std::cout << "PASS: nested savepoints, stale DBI reuse and environment identity\n";

    ulmdb::transaction survivor;
    ulmdb::cursor survivor_cursor;
    ulmdb::database kept;
    {
        ulmdb::environment temporary(files.path("kept"));
        auto write = temporary.begin_write();
        kept = write.open_database("items", ulmdb::database_kind::kv, true);
        write.put(kept, "key", "owned"); write.commit();
        survivor = temporary.begin_read();
        survivor_cursor = survivor.open_cursor(kept);
        temporary.close();
    }
    require(survivor.get(kept, "key") == "owned", "transaction retains native environment");
    auto copied = survivor_cursor.first();
    survivor.abort();
    expect_error(MDB_BAD_TXN, [&] { survivor_cursor.next(); });
    require(copied->second == "owned", "cursor records own their bytes");
    ulmdb::transaction orphaned_child;
    ulmdb::cursor orphaned_cursor;
    {
        ulmdb::environment temporary(files.path("parent"));
        auto parent = temporary.begin_write();
        auto local = parent.open_database("items", ulmdb::database_kind::kv, true);
        orphaned_child = parent.nested();
        orphaned_child.put(local, "key", "private");
        orphaned_cursor = orphaned_child.open_cursor(local);
    }
    expect_error(MDB_BAD_TXN, [&] { orphaned_child.main_database(); });
    expect_error(MDB_BAD_TXN, [&] { orphaned_cursor.first(); });
    orphaned_child.abort(); orphaned_cursor.close();
    std::cout << "PASS: environment/parent destruction safely invalidates live dependants\n";
}
