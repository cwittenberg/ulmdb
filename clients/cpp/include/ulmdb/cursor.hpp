#ifndef ULMDB_CPP_CURSOR_HPP
#define ULMDB_CPP_CURSOR_HPP
#include "database.hpp"
#include <iterator>

namespace ulmdb {
class cursor {
    std::shared_ptr<detail::cursor_state> state_;
    explicit cursor(std::shared_ptr<detail::cursor_state> state) : state_(std::move(state)) {}
    friend class transaction;
public:
    class iterator {
        std::shared_ptr<detail::cursor_state> state_;
        std::optional<record> value_;
        iterator(std::shared_ptr<detail::cursor_state> state, std::optional<record> value)
            : state_(std::move(state)), value_(std::move(value)) {}
        friend class cursor;
    public:
        using iterator_category = std::input_iterator_tag;
        using value_type = record;
        using difference_type = std::ptrdiff_t;
        using reference = const record&;
        using pointer = const record*;
        iterator() = default;
        reference operator*() const {
            if (!value_) throw error(MDB_NOTFOUND, "iterator is exhausted");
            return *value_;
        }
        pointer operator->() const { return &**this; }
        iterator& operator++() {
            if (!value_) throw error(MDB_NOTFOUND, "iterator is exhausted");
            value_ = detail::cursor_read(state_, MDB_NEXT);
            return *this;
        }
        iterator operator++(int) { iterator previous = *this; ++*this; return previous; }
        friend bool operator==(const iterator& left, const iterator& right) {
            if (!left.value_ || !right.value_) return !left.value_ && !right.value_;
            return left.state_ == right.state_ && left.value_ == right.value_;
        }
        friend bool operator!=(const iterator& left, const iterator& right) { return !(left == right); }
    };
    cursor() = default;
    cursor(const cursor&) = delete;
    cursor& operator=(const cursor&) = delete;
    cursor(cursor&&) noexcept = default;
    cursor& operator=(cursor&& other) noexcept {
        if (this != &other) { if (state_) state_->close_noexcept(); state_ = std::move(other.state_); }
        return *this;
    }
    ~cursor() noexcept { if (state_) state_->close_noexcept(); }
    void close() {
        if (state_ && state_->handle) { state_->transaction->require_thread(); state_->close_noexcept(); }
    }
    std::optional<record> first() { return detail::cursor_read(state_, MDB_FIRST); }
    std::optional<record> last() { return detail::cursor_read(state_, MDB_LAST); }
    std::optional<record> next() { return detail::cursor_read(state_, MDB_NEXT); }
    std::optional<record> previous() { return detail::cursor_read(state_, MDB_PREV); }
    std::optional<record> current() { return detail::cursor_read(state_, MDB_GET_CURRENT); }
    std::optional<record> seek(std::string_view key) { return detail::cursor_read(state_, MDB_SET_RANGE, key); }
    std::optional<record> seek_exact(std::string_view key) { return detail::cursor_read(state_, MDB_SET_KEY, key); }
    std::optional<record> seek_both(std::string_view key, std::string_view value, bool range = false) {
        return detail::cursor_read(state_, range ? MDB_GET_BOTH_RANGE : MDB_GET_BOTH, key, value);
    }
    std::optional<record> first_duplicate() { return detail::cursor_read(state_, MDB_FIRST_DUP); }
    std::optional<record> last_duplicate() { return detail::cursor_read(state_, MDB_LAST_DUP); }
    std::optional<record> next_duplicate() { return detail::cursor_read(state_, MDB_NEXT_DUP); }
    std::optional<record> previous_duplicate() { return detail::cursor_read(state_, MDB_PREV_DUP); }
    std::optional<record> next_key() { return detail::cursor_read(state_, MDB_NEXT_NODUP); }
    std::optional<record> previous_key() { return detail::cursor_read(state_, MDB_PREV_NODUP); }
    std::size_t duplicate_count() const {
        if (!state_) throw error(MDB_BAD_TXN, "cursor is empty");
        state_->usable();
        std::size_t count;
        detail::check(mdb_cursor_count(state_->handle, &count), "count cursor duplicates");
        return count;
    }
    bool put(std::string_view key, std::string_view value, put_options options = {}) {
        if (!state_) throw error(MDB_BAD_TXN, "cursor is empty");
        state_->usable(); state_->transaction->writable();
        MDB_val native_key = detail::input(key), native_value = detail::input(value);
        int result = mdb_cursor_put(state_->handle, &native_key, &native_value, detail::put_flags(options));
        if (result == MDB_KEYEXIST) return false;
        detail::check(result, "write cursor"); return true;
    }
    void replace(std::string_view value) {
        auto item = current();
        if (!item) throw error(MDB_NOTFOUND, "cursor has no current record");
        state_->transaction->writable();
        MDB_val key = detail::input(item->first), native_value = detail::input(value);
        detail::check(mdb_cursor_put(state_->handle, &key, &native_value, MDB_CURRENT), "replace cursor value");
    }
    void erase(bool all_duplicates = false) {
        if (!state_) throw error(MDB_BAD_TXN, "cursor is empty");
        state_->usable(); state_->transaction->writable();
        detail::check(mdb_cursor_del(state_->handle, all_duplicates ? MDB_NODUPDATA : 0), "delete cursor record");
    }
    iterator begin() { return iterator(state_, first()); }
    iterator end() const noexcept { return {}; }
};
}
#endif
