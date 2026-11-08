#ifndef ULMDB_CPP_TRANSACTION_HPP
#define ULMDB_CPP_TRANSACTION_HPP
#include "cursor.hpp"
#include <algorithm>

namespace ulmdb {
class environment;
class transaction {
    std::shared_ptr<detail::transaction_state> state_;
    explicit transaction(std::shared_ptr<detail::transaction_state> state) : state_(std::move(state)) {}
    friend class environment;
    detail::transaction_state& state() const {
        if (!state_) throw error(MDB_BAD_TXN, "transaction is empty");
        state_->usable(); return *state_;
    }
    database open(std::string name, bool main, database_kind kind, bool create) {
        auto& owner = state();
        MDB_dbi handle = 0;
        int result = ulmdb_dbi_open(owner.handle, main ? nullptr : name.c_str(), static_cast<unsigned>(kind), 0, &handle);
        std::shared_ptr<detail::database_state> db;
        if (result == MDB_NOTFOUND && create) {
            owner.writable();
            db = std::make_shared<detail::database_state>(owner.environment, kind, name, main);
            owner.created.reserve(owner.created.size() + 1);
            detail::check(ulmdb_dbi_open(owner.handle, main ? nullptr : name.c_str(), static_cast<unsigned>(kind), MDB_CREATE, &handle), "create database");
            db->handle = handle;
            owner.created.push_back(db);
        } else detail::check(result, "open database");
        std::lock_guard<std::mutex> lock(owner.environment->registry_mutex);
        if (!db) {
            auto registered = owner.environment->databases.find(handle);
            if (registered != owner.environment->databases.end()) db = registered->second.lock();
            if (db && !db->valid.load()) db.reset();
            // Provisional controls are retained until the outer transaction ends,
            // including when a previous registry allocation threw bad_alloc.
            for (auto ancestor = state_; !db && ancestor; ancestor = ancestor->parent)
                for (auto& pending : ancestor->created)
                    if (pending->handle == handle && pending->valid.load()) { db = pending; break; }
            if (!db) {
                db = std::make_shared<detail::database_state>(owner.environment, kind, name, main);
                db->handle = handle;
            }
        }
        owner.environment->databases[handle] = db;
        return database(std::move(db));
    }
public:
    transaction() = default;
    transaction(const transaction&) = delete;
    transaction& operator=(const transaction&) = delete;
    transaction(transaction&&) noexcept = default;
    transaction& operator=(transaction&& other) noexcept {
        if (this != &other) { if (state_) state_->abort_noexcept(); state_ = std::move(other.state_); }
        return *this;
    }
    ~transaction() noexcept { if (state_) state_->abort_noexcept(); }
    bool readonly() const { return state().readonly; }
    database main_database() { return open({}, true, database_kind::kv, false); }
    database open_database(std::string_view name, database_kind kind = database_kind::kv, bool create = false) {
        auto owned_name = detail::text(name, "database name contains NUL");
        if (owned_name.empty()) throw error(EINVAL, "named database name is empty");
        return open(std::move(owned_name), false, kind, create);
    }
    transaction nested() {
        auto& owner = state(); owner.writable();
        auto child = std::make_shared<detail::transaction_state>(owner.environment, false, state_);
        detail::check(mdb_txn_begin(owner.environment->handle, owner.handle, 0, &child->handle), "begin nested transaction");
        owner.child = child;
        return transaction(std::move(child));
    }
    void commit() { state().commit(); }
    void abort() {
        if (state_ && state_->handle) { state_->require_thread(); state_->abort_noexcept(); }
    }
    std::optional<std::string> get(const database& db, std::string_view key) const {
        auto& owner = state(); MDB_dbi handle = owner.database(db.state_);
        MDB_val native_key = detail::input(key), value{0, nullptr};
        int result = mdb_get(owner.handle, handle, &native_key, &value);
        if (result == MDB_NOTFOUND) return std::nullopt;
        detail::check(result, "get record"); return detail::owned(value);
    }
    bool put(const database& db, std::string_view key, std::string_view value, put_options options = {}) {
        auto& owner = state(); owner.writable(); MDB_dbi handle = owner.database(db.state_);
        MDB_val native_key = detail::input(key), native_value = detail::input(value);
        int result = mdb_put(owner.handle, handle, &native_key, &native_value, detail::put_flags(options));
        if (result == MDB_KEYEXIST) return false;
        detail::check(result, "put record"); return true;
    }
    bool erase(const database& db, std::string_view key, std::optional<std::string_view> value = std::nullopt) {
        auto& owner = state(); owner.writable(); MDB_dbi handle = owner.database(db.state_);
        MDB_val native_key = detail::input(key), native_value = value ? detail::input(*value) : MDB_val{0, nullptr};
        int result = mdb_del(owner.handle, handle, &native_key, value ? &native_value : nullptr);
        if (result == MDB_NOTFOUND) return false;
        detail::check(result, "delete record"); return true;
    }
    cursor open_cursor(const database& db) {
        auto& owner = state(); MDB_dbi handle = owner.database(db.state_);
        auto result = std::make_shared<detail::cursor_state>(state_, db.state_);
        owner.cursors.erase(std::remove_if(owner.cursors.begin(), owner.cursors.end(),
            [](const auto& cursor) { return cursor.expired(); }), owner.cursors.end());
        owner.cursors.reserve(owner.cursors.size() + 1);
        detail::check(mdb_cursor_open(owner.handle, handle, &result->handle), "open cursor");
        owner.cursors.push_back(result);
        return cursor(std::move(result));
    }
    std::size_t size(const database& db) const {
        auto& owner = state(); MDB_stat statistics;
        detail::check(mdb_stat(owner.handle, owner.database(db.state_), &statistics), "database statistics");
        return statistics.ms_entries;
    }
    void prefix_put(const database& db, std::string_view cidr, std::string_view value) {
        auto& owner = state(); owner.writable(); MDB_dbi handle = owner.database(db.state_);
        auto prefix = detail::text(cidr, "prefix contains NUL"); MDB_val native_value = detail::input(value);
        detail::check(ulmdb_prefix_put(owner.handle, handle, prefix.c_str(), &native_value), "put prefix");
    }
    std::optional<std::string> prefix_get(const database& db, std::string_view cidr) const {
        auto& owner = state(); MDB_dbi handle = owner.database(db.state_);
        auto prefix = detail::text(cidr, "prefix contains NUL"); MDB_val value{0, nullptr};
        int result = ulmdb_prefix_get(owner.handle, handle, prefix.c_str(), &value);
        if (result == MDB_NOTFOUND) return std::nullopt;
        detail::check(result, "get prefix"); return detail::owned(value);
    }
    bool prefix_erase(const database& db, std::string_view cidr) {
        auto& owner = state(); owner.writable(); MDB_dbi handle = owner.database(db.state_);
        auto prefix = detail::text(cidr, "prefix contains NUL");
        int result = ulmdb_prefix_delete(owner.handle, handle, prefix.c_str());
        if (result == MDB_NOTFOUND) return false;
        detail::check(result, "delete prefix"); return true;
    }
    std::optional<prefix_match> longest_match(const database& db, std::string_view address) const {
        auto& owner = state(); MDB_dbi handle = owner.database(db.state_);
        auto input_address = detail::text(address, "address contains NUL");
        MDB_val cidr{0, nullptr}, value{0, nullptr};
        int result = ulmdb_longest_match(owner.handle, handle, input_address.c_str(), &cidr, &value);
        if (result == MDB_NOTFOUND) return std::nullopt;
        detail::check(result, "longest-prefix match"); return prefix_match{detail::owned(cidr), detail::owned(value)};
    }
};
}
#endif
