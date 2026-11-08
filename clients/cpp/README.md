# ULMDB C++ client

C++17 headers over `libulmdb_c`. Requires Cargo, CMake 3.16+ and a C++17 compiler.

## Build

```sh
# From the repository root:
cargo build -p ulmdb-c --locked
cmake -S clients/cpp -B target/ulmdb-cpp
cmake --build target/ulmdb-cpp
```

- External library: pass `-DULMDB_C_LIBRARY=/absolute/path/to/libulmdb_c.so` (`.dylib` on macOS).

## Use

```cmake
# In your CMake project:
add_subdirectory(path/to/ulmdb/clients/cpp ulmdb-cpp)
target_link_libraries(your_app PRIVATE ulmdb::cpp)
```

```cpp
#include <ulmdb/ulmdb.hpp>
#include <iostream>

int main() {
    ulmdb::environment env("./example-db");
    auto write = env.begin_write();
    auto people = write.open_database("people", ulmdb::database_kind::kv, true);
    write.put(people, "alice", "Alice");
    write.commit();

    auto read = env.begin_read();
    if (auto value = read.get(people, "alice"))
        std::cout << *value << '\n';
}
```

- Writes require `commit()`; destruction aborts uncommitted transactions.
- Keep transactions and cursors on their creating thread.
- [Headers](include/ulmdb/) · [KV example](examples/basic.cpp) · [IPv4/IPv6 example](examples/routes.cpp) · [C API and limits](../c/README.md)

## Test

```sh
ctest --test-dir target/ulmdb-cpp --output-on-failure
./target/ulmdb-cpp/ulmdb_cpp_basic /tmp/new-ulmdb-cpp
./target/ulmdb-cpp/ulmdb_cpp_routes /tmp/new-ulmdb-cpp-routes
```
