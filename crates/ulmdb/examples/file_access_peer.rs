use ulmdb::{DatabaseKind, Environment, PageSize, PublicationMode, ReadAhead, ResidencyPolicy};
use ulmdb_storage::access::FileAccess;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = match args.get(3).map(String::as_str) {
        Some("cohort") => PublicationMode::Cohort,
        Some("indexed") => PublicationMode::Indexed,
        _ => PublicationMode::Legacy,
    };
    let path = std::path::Path::new(&args[2]);
    let gid = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(1001);
    match args[1].as_str() {
        "create" | "empty" => {
            let e = Environment::create_with_file_access(
                path,
                mode,
                PageSize::DEFAULT,
                ResidencyPolicy::Demand,
                ReadAhead::Normal,
                None,
                FileAccess::shared_group(0o660, gid).unwrap(),
            )
            .unwrap();
            if args[1] == "empty" {
                return;
            }
            let mut w = e.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"k", b"A").unwrap();
            w.commit().unwrap();
        }
        "write" => {
            let e = Environment::open_with_file_access(
                path,
                FileAccess::shared_group(0o660, gid).unwrap(),
            )
            .unwrap();
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"k", b"B").unwrap();
            w.commit().unwrap();
        }
        "read-a" | "read-b" => {
            let r = ulmdb::ReadTxn::open_coordinated_snapshot(path).unwrap();
            assert_eq!(
                r.get(b"kv", b"k").unwrap(),
                Some(if args[1] == "read-a" {
                    b"A".as_slice()
                } else {
                    b"B".as_slice()
                })
            );
        }
        _ => panic!("role"),
    };
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    println!(
        "{}",
        status.lines().find(|s| s.starts_with("Uid:")).unwrap()
    );
}
