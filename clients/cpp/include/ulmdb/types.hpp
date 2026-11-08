#ifndef ULMDB_CPP_TYPES_HPP
#define ULMDB_CPP_TYPES_HPP
#include "error.hpp"
#include <cerrno>
#include <optional>
#include <string>
#include <string_view>
#include <utility>

namespace ulmdb {
enum class database_kind : unsigned { kv = ULMDB_KV, duplicates = ULMDB_DUPLICATES, ipv4 = ULMDB_IPV4, ipv6 = ULMDB_IPV6 };
using record = std::pair<std::string, std::string>;
struct prefix_match { std::string cidr, value; };
struct put_options { bool overwrite = true; bool no_duplicate_data = false; };
struct environment_options {
    std::size_t map_size = 0; // Adopt existing capacity; new environments use the C default.
    unsigned max_databases = 64;
    unsigned max_readers = 126;
    unsigned flags = MDB_NOTLS;
    bool readonly = false;
    mdb_mode_t mode = 0600;
};
namespace detail {
inline MDB_val input(std::string_view value) noexcept {
    return MDB_val{value.size(), const_cast<char*>(value.data())};
}
inline std::string owned(MDB_val value) {
    return value.mv_size ? std::string(static_cast<const char*>(value.mv_data), value.mv_size) : std::string{};
}
inline std::string text(std::string_view value, const char* operation) {
    if (value.find('\0') != std::string_view::npos) throw error(EINVAL, operation);
    return std::string(value);
}
inline unsigned put_flags(put_options options) noexcept {
    return (options.overwrite ? 0u : MDB_NOOVERWRITE) | (options.no_duplicate_data ? MDB_NODUPDATA : 0u);
}
}
}
#endif
