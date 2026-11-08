/* Shared control blocks keep native owners alive and invalidate dependants. */
#ifndef ULMDB_CPP_DETAIL_STATE_HPP
#define ULMDB_CPP_DETAIL_STATE_HPP
#include "../types.hpp"
#include <atomic>
#include <exception>
#include <memory>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <vector>

namespace ulmdb::detail {
struct database_state;
struct cursor_state;
struct environment_state {
    MDB_env* handle = nullptr;
    std::mutex registry_mutex;
    std::unordered_map<MDB_dbi, std::weak_ptr<database_state>> databases;
    ~environment_state() noexcept { if (handle) mdb_env_close(handle); }
};
struct database_state {
    std::shared_ptr<environment_state> environment;
    MDB_dbi handle = 0;
    database_kind kind;
    std::string name;
    bool main;
    std::atomic<bool> valid{true};
    database_state(std::shared_ptr<environment_state> owner, database_kind type, std::string label, bool unnamed)
        : environment(std::move(owner)), kind(type), name(std::move(label)), main(unnamed) {}
};
struct transaction_state : std::enable_shared_from_this<transaction_state> {
    std::shared_ptr<environment_state> environment;
    std::shared_ptr<transaction_state> parent;
    std::weak_ptr<transaction_state> child;
    std::vector<std::weak_ptr<cursor_state>> cursors;
    std::vector<std::shared_ptr<database_state>> created;
    MDB_txn* handle = nullptr;
    const std::thread::id thread = std::this_thread::get_id();
    bool readonly;
    transaction_state(std::shared_ptr<environment_state> owner, bool read,
                      std::shared_ptr<transaction_state> ancestor = {})
        : environment(std::move(owner)), parent(std::move(ancestor)), readonly(read) {}
    ~transaction_state() noexcept;
    void require_thread() const {
        if (thread != std::this_thread::get_id()) throw error(MDB_BAD_TXN, "transaction belongs to another thread");
    }
    void usable() const {
        require_thread();
        if (!handle) throw error(MDB_BAD_TXN, "transaction has ended");
        if (auto nested = child.lock(); nested && nested->handle)
            throw error(MDB_BAD_TXN, "parent transaction has an active child");
    }
    void writable() const {
        usable();
        if (readonly) throw error(EACCES, "transaction is read-only");
    }
    MDB_dbi database(const std::shared_ptr<database_state>& db) const {
        usable();
        if (!db || !db->valid.load() || db->environment.get() != environment.get())
            throw error(MDB_BAD_DBI, "database does not belong to this environment or was rolled back");
        return db->handle;
    }
    void close_cursors() noexcept;
    void abort_noexcept() noexcept;
    void commit();
};
struct cursor_state {
    std::shared_ptr<transaction_state> transaction;
    std::shared_ptr<database_state> database;
    MDB_cursor* handle = nullptr;
    cursor_state(std::shared_ptr<transaction_state> owner, std::shared_ptr<database_state> db)
        : transaction(std::move(owner)), database(std::move(db)) {}
    ~cursor_state() noexcept { close_noexcept(); }
    void usable() const {
        transaction->database(database);
        if (!handle) throw error(MDB_BAD_TXN, "cursor has closed");
    }
    void close_noexcept() noexcept {
        if (!handle) return;
        if (transaction->thread != std::this_thread::get_id()) std::terminate();
        mdb_cursor_close(handle);
        handle = nullptr;
    }
};
inline void transaction_state::close_cursors() noexcept {
    for (auto& entry : cursors) if (auto cursor = entry.lock()) cursor->close_noexcept();
    cursors.clear();
}
inline void transaction_state::abort_noexcept() noexcept {
    if (!handle) return;
    if (thread != std::this_thread::get_id()) std::terminate();
    if (auto nested = child.lock()) nested->abort_noexcept();
    close_cursors();
    for (auto& db : created) db->valid.store(false);
    mdb_txn_abort(handle);
    handle = nullptr;
    created.clear();
    if (parent) parent->child.reset();
}
inline transaction_state::~transaction_state() noexcept { abort_noexcept(); }
inline void transaction_state::commit() {
    usable();
    if (parent) parent->created.reserve(parent->created.size() + created.size());
    close_cursors();
    MDB_txn* committing = handle;
    handle = nullptr; // The C API consumes commit handles, including error outcomes.
    int result = mdb_txn_commit(committing);
    if (parent) parent->child.reset();
    if (result) for (auto& db : created) db->valid.store(false);
    else if (parent) parent->created.insert(parent->created.end(), created.begin(), created.end());
    created.clear();
    check(result, "commit transaction");
}
inline std::optional<record> cursor_read(const std::shared_ptr<cursor_state>& cursor,
                                        MDB_cursor_op operation,
                                        std::string_view key = {}, std::string_view value = {}) {
    if (!cursor) throw error(MDB_BAD_TXN, "cursor is empty");
    cursor->usable();
    MDB_val native_key = input(key), native_value = input(value);
    int result = mdb_cursor_get(cursor->handle, &native_key, &native_value, operation);
    if (result == MDB_NOTFOUND) return std::nullopt;
    check(result, "read cursor");
    if (operation == MDB_FIRST_DUP || operation == MDB_LAST_DUP) {
        check(mdb_cursor_get(cursor->handle, &native_key, &native_value, MDB_GET_CURRENT), "read duplicate cursor");
    }
    return record{owned(native_key), owned(native_value)};
}
}
#endif
