#ifndef ULMDB_CPP_ENVIRONMENT_HPP
#define ULMDB_CPP_ENVIRONMENT_HPP
#include "transaction.hpp"

namespace ulmdb {
class environment {
    std::shared_ptr<detail::environment_state> state_;
    transaction begin(bool readonly) const {
        if (!state_) throw error(EINVAL, "environment has closed");
        auto result = std::make_shared<detail::transaction_state>(state_, readonly);
        detail::check(mdb_txn_begin(state_->handle, nullptr, readonly ? MDB_RDONLY : 0, &result->handle), "begin transaction");
        return transaction(std::move(result));
    }
public:
    explicit environment(std::string_view path, environment_options options = {})
        : state_(std::make_shared<detail::environment_state>()) {
        auto native_path = detail::text(path, "environment path contains NUL");
        detail::check(mdb_env_create(&state_->handle), "create environment");
        detail::check(mdb_env_set_maxdbs(state_->handle, options.max_databases), "set database limit");
        detail::check(mdb_env_set_maxreaders(state_->handle, options.max_readers), "set reader limit");
        detail::check(mdb_env_set_mapsize(state_->handle, options.map_size), "set map capacity");
        detail::check(mdb_env_open(state_->handle, native_path.c_str(),
                      options.flags | (options.readonly ? MDB_RDONLY : 0), options.mode), "open environment");
    }
    environment(const environment&) = delete;
    environment& operator=(const environment&) = delete;
    environment(environment&&) noexcept = default;
    environment& operator=(environment&&) noexcept = default;
    transaction begin_read() const { return begin(true); }
    transaction begin_write() const { return begin(false); }
    void close() noexcept { state_.reset(); }
    void sync(bool force = true) const {
        if (!state_) throw error(EINVAL, "environment has closed");
        detail::check(mdb_env_sync(state_->handle, force ? 1 : 0), "sync environment");
    }
};
}
#endif
