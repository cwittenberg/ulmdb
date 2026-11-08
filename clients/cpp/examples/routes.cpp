#include <ulmdb/ulmdb.hpp>
#include <iostream>

int main(int argc, char** argv) {
    if (argc != 2) { std::cerr << "usage: ulmdb_cpp_routes DATABASE_DIRECTORY\n"; return 2; }
    try {
        ulmdb::environment env(argv[1]);
        auto write = env.begin_write();
        auto v4 = write.open_database("routes4", ulmdb::database_kind::ipv4, true);
        auto v6 = write.open_database("routes6", ulmdb::database_kind::ipv6, true);
        write.prefix_put(v4, "0.0.0.0/0", "default");
        write.prefix_put(v4, "192.0.2.0/24", "documentation");
        write.prefix_put(v6, "2001:db8::/32", "documentation-v6");
        write.prefix_put(v6, "2001:db8:1::/48", "specific-v6");
        write.commit();
        auto read = env.begin_read();
        auto ipv4 = read.longest_match(v4, "192.0.2.42");
        auto ipv6 = read.longest_match(v6, "2001:db8:1::42");
        if (ipv4) std::cout << ipv4->cidr << " = " << ipv4->value << '\n';
        if (ipv6) std::cout << ipv6->cidr << " = " << ipv6->value << '\n';
        return 0;
    } catch (const ulmdb::error& error) {
        std::cerr << error.what() << " (" << error.code() << ")\n"; return 1;
    }
}
