#include <ulmdb/ulmdb.hpp>
#include <iostream>

int main(int argc, char** argv) {
    if (argc != 2) { std::cerr << "usage: ulmdb_cpp_basic DATABASE_DIRECTORY\n"; return 2; }
    try {
        ulmdb::environment env(argv[1]);
        auto write = env.begin_write();
        auto people = write.open_database("people", ulmdb::database_kind::kv, true);
        write.put(people, "alice", "Alice");
        write.put(people, "bob", "Bob");
        write.commit();
        auto read = env.begin_read();
        for (const auto& [key, value] : read.open_cursor(people))
            std::cout << key << " = " << value << '\n';
        return 0;
    } catch (const ulmdb::error& error) {
        std::cerr << error.what() << " (" << error.code() << ")\n"; return 1;
    }
}
