//! Initialize only an empty physical backend; every business operation is C.
use ulmdb::{Environment, PublicationMode};
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    assert_eq!(args.len(), 3);
    let mode = match args[1].to_str().unwrap() {
        "legacy" => PublicationMode::Legacy,
        "cohort" => PublicationMode::Cohort,
        "indexed" => PublicationMode::Indexed,
        _ => panic!("unknown physical backend"),
    };
    let env = Environment::create_with_publication(&args[2], mode).unwrap();
    let read = env.begin_read();
    assert_eq!(read.sequence(), 0);
    assert!(read.database_names().unwrap().is_empty());
    println!("empty backend={mode:?} sequence=0 databases=0");
}
