#include "support.hpp"
#include <memory>

namespace detail = ulmdb::detail;

void test_routes_and_c_interoperability() {
    fixture files;
    const auto path = files.path("interop");
    const std::string binary_key("key\0\xff", 5), binary_value("value\0\xff", 7);
    {
        ulmdb::environment env(path);
        auto write = env.begin_write();
        auto kv = write.open_database("bytes", ulmdb::database_kind::kv, true);
        auto v4 = write.open_database("v4", ulmdb::database_kind::ipv4, true);
        auto v6 = write.open_database("v6", ulmdb::database_kind::ipv6, true);
        write.put(kv, binary_key, binary_value);
        write.prefix_put(v4, "0.0.0.0/0", "default");
        write.prefix_put(v4, "192.0.2.0/24", binary_value);
        write.prefix_put(v4, "192.0.2.128/25", "specific");
        write.prefix_put(v6, "2001:db8::/32", "v6");
        require(write.prefix_get(v4, "192.0.2.0/24") == binary_value, "IPv4 exact prefix");
        auto first = write.longest_match(v4, "192.0.2.200");
        require(first && first->cidr == "192.0.2.128/25" && first->value == "specific", "IPv4 longest prefix");
        require(write.longest_match(v6, "2001:db8:1::1")->cidr == "2001:db8::/32", "IPv6 longest prefix");
        require(first->cidr == "192.0.2.128/25", "matched CIDR is owned across later matches");
        require(write.prefix_erase(v4, "192.0.2.128/25"), "prefix deletion");
        require(!write.prefix_erase(v4, "192.0.2.128/25"), "missing prefix deletion");
        require(write.longest_match(v4, "192.0.2.200")->cidr == "192.0.2.0/24", "deletion falls back to parent");
        expect_error(EINVAL, [&] { write.prefix_put(v4, "192.0.2.1/24", "noncanonical"); });
        write.commit();
    }
    MDB_env* raw_env = nullptr;
    detail::check(mdb_env_create(&raw_env), "C env create");
    std::unique_ptr<MDB_env, decltype(&mdb_env_close)> native_env(raw_env, mdb_env_close);
    detail::check(mdb_env_set_maxdbs(raw_env, 16), "C max dbs");
    detail::check(mdb_env_open(raw_env, path.c_str(), MDB_NOTLS, 0600), "C env open");
    MDB_txn* raw_txn = nullptr;
    detail::check(mdb_txn_begin(raw_env, nullptr, MDB_RDONLY, &raw_txn), "C read begin");
    std::unique_ptr<MDB_txn, decltype(&mdb_txn_abort)> native_txn(raw_txn, mdb_txn_abort);
    MDB_dbi kv, v4;
    detail::check(mdb_dbi_open(raw_txn, "bytes", 0, &kv), "C db open");
    detail::check(ulmdb_dbi_open(raw_txn, "v4", ULMDB_IPV4, 0, &v4), "C IPv4 open");
    MDB_val key = ulmdb::detail::input(binary_key), value{0, nullptr}, cidr{0, nullptr};
    detail::check(mdb_get(raw_txn, kv, &key, &value), "C binary get");
    require(ulmdb::detail::owned(value) == binary_value, "C reads C++ binary record");
    detail::check(ulmdb_longest_match(raw_txn, v4, "192.0.2.200", &cidr, &value), "C longest match");
    require(ulmdb::detail::owned(cidr) == "192.0.2.0/24" && ulmdb::detail::owned(value) == binary_value,
            "C reads C++ prefix and binary payload");
    native_txn.reset();
    detail::check(mdb_txn_begin(raw_env, nullptr, 0, &raw_txn), "C write begin");
    native_txn.reset(raw_txn);
    key = ulmdb::detail::input("from-c"); value = ulmdb::detail::input(binary_value);
    detail::check(mdb_put(raw_txn, kv, &key, &value, 0), "C binary write");
    MDB_dbi v6;
    detail::check(ulmdb_dbi_open(raw_txn, "v6", ULMDB_IPV6, 0, &v6), "C IPv6 open");
    detail::check(ulmdb_prefix_put(raw_txn, v6, "2001:db8:1::/48", &value), "C IPv6 write");
    detail::check(mdb_txn_commit(native_txn.release()), "C commit");
    native_env.reset();
    {
        ulmdb::environment env(path);
        auto read = env.begin_read();
        auto bytes = read.open_database("bytes");
        auto routes = read.open_database("v6", ulmdb::database_kind::ipv6);
        require(read.get(bytes, "from-c") == binary_value, "C++ reads C binary record");
        auto match = read.longest_match(routes, "2001:db8:1::1234");
        require(match && match->cidr == "2001:db8:1::/48" && match->value == binary_value,
                "C++ reads C IPv6 prefix and binary payload");
    }
    std::cout << "PASS: IPv4/IPv6 indexes and bidirectional C ABI interoperability\n";
}
