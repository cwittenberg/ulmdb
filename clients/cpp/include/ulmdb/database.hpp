#ifndef ULMDB_CPP_DATABASE_HPP
#define ULMDB_CPP_DATABASE_HPP
#include "detail/state.hpp"

namespace ulmdb {
class transaction;
class database {
    std::shared_ptr<detail::database_state> state_;
    explicit database(std::shared_ptr<detail::database_state> state) : state_(std::move(state)) {}
    friend class transaction;
public:
    database() = default;
    bool valid() const noexcept { return state_ && state_->valid.load(); }
    database_kind kind() const {
        if (!valid()) throw error(MDB_BAD_DBI, "database is empty or was rolled back");
        return state_->kind;
    }
};
}
#endif
