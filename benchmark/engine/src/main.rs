use std::path::Path;
use ulmdb_engine_benchmark::{Result, dataset::Config, run};
fn execute() -> Result<String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice(){[mode,path]if mode=="bench"=>run::execute(Path::new(path),Config::default(),true),[mode,path]if mode=="smoke"=>run::execute(Path::new(path),Config::smoke(),false),_=>Err("usage: ulmdb-engine-benchmark bench|smoke NEW_DIRECTORY (backend chosen by build feature)".into())}
}
fn main() {
    match execute() {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("engine benchmark failed: {e}");
            std::process::exit(1);
        }
    }
}
