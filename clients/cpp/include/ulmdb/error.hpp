#ifndef ULMDB_CPP_ERROR_HPP
#define ULMDB_CPP_ERROR_HPP
#include <ulmdb.h>
#include <stdexcept>
#include <string>

namespace ulmdb {
class error : public std::runtime_error {
    int code_;
public:
    error(int code, const std::string& operation)
        : std::runtime_error(operation + ": " + mdb_strerror(code)), code_(code) {}
    int code() const noexcept { return code_; }
};
namespace detail {
inline void check(int result, const char* operation) {
    if (result) throw error(result, operation);
}
}
}
#endif
