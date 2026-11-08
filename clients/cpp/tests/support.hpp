#ifndef ULMDB_CPP_TEST_SUPPORT_HPP
#define ULMDB_CPP_TEST_SUPPORT_HPP
#include <ulmdb/ulmdb.hpp>
#include <chrono>
#include <filesystem>
#include <iostream>
#include <stdexcept>
#include <string>

inline void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}
template<class Function> void expect_error(int code, Function function) {
    try { function(); }
    catch (const ulmdb::error& error) { require(error.code() == code, error.what()); return; }
    throw std::runtime_error("expected an ulmdb::error");
}
struct fixture {
    std::filesystem::path root;
    fixture() {
        auto stamp = std::chrono::steady_clock::now().time_since_epoch().count();
        for (unsigned attempt = 0; ; ++attempt) {
            root = std::filesystem::temp_directory_path() /
                ("ulmdb-cpp-" + std::to_string(stamp) + "-" + std::to_string(attempt));
            if (std::filesystem::create_directory(root)) break;
        }
    }
    ~fixture() { std::error_code error; std::filesystem::remove_all(root, error); }
    std::string path(const char* name) const { return (root / name).string(); }
};
void test_lifetimes();
void test_routes_and_c_interoperability();
#endif
