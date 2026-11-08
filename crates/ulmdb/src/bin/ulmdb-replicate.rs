//! Framed file transport. Move frames over an authenticated/private transport as
//! required by deployment; this CLI does not claim a network service or TLS.
#[path = "replicate/keys.rs"]
mod keys;
#[path = "common/residency.rs"]
mod residency_options;
use std::{
    error::Error,
    fs::{File, OpenOptions},
    path::Path,
};
use ulmdb::{
    Environment,
    replication::{self, Cursor, TransferLimits},
};
fn execute() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let (policy, args) = residency_options::parse(&arguments)?;
    match args {
        [cmd,db,keyfile,id] if cmd=="keygen"=>{
            let env=Environment::open_with_residency(db, policy)?;let cursor=Cursor::from_read(&env.begin_read())?;
            keys::generate(Path::new(keyfile),cursor,id.parse()?)?;
            println!("created scoped publisher key {keyfile}");
        },
        [cmd,db,keyfile,output] | [cmd,db,keyfile,output,_] if cmd=="send"=>{
            let env=Environment::open_with_residency(db, policy)?;let key=keys::load(Path::new(keyfile))?;
            let read=env.begin_read();let base=args.get(4).map(|name|env.open_retained(name)).transpose()?;
            let mut options=OpenOptions::new();options.create_new(true).write(true);
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt;options.mode(0o600); }
            let mut file=options.open(output)?;
            let report=replication::send_snapshot(&env,&read,base.as_ref(),&key,&mut file)?;
            ulmdb_platform::sync_file(&file)?;
            println!("files={} payload_bytes={} wire_bytes={} sequence={} generation={:032x}",
                report.files,report.payload_bytes,report.wire_bytes,report.target.sequence,report.target.generation()?);
        },
        [cmd,frame,keyfile,stage,target] if cmd=="bootstrap"=>{
            let key=keys::load(Path::new(keyfile))?;let mut input=File::open(frame)?;
            let verified=replication::receive(&mut input,&key,TransferLimits::default(),stage)?;
            let env=replication::bootstrap_with_residency(&verified,target,policy)?;
            let cursor=Cursor::from_read(&env.begin_read())?;
            println!("replica=true sequence={} generation={:032x}",cursor.sequence,cursor.generation()?);
        },
        [cmd,db,frame,keyfile,stage] if cmd=="apply"=>{
            let key=keys::load(Path::new(keyfile))?;let mut input=File::open(frame)?;
            let verified=replication::receive(&mut input,&key,TransferLimits::default(),stage)?;
            let env=Environment::open_with_residency(db, policy)?;let report=env.apply_replication(&verified)?;
            println!("outcome={:?} imported_files={} imported_bytes={} sequence={} generation={:032x}",
                report.outcome,report.imported_files,report.imported_bytes,report.cursor.sequence,report.cursor.generation()?);
        },
        _=>return Err("usage: ulmdb-replicate [--residency demand|prefault:BYTES|locked:BYTES] keygen DB NEW_KEY_FILE KEY_ID | send DB KEY_FILE NEW_FRAME [BASE_PIN] | bootstrap FRAME KEY_FILE STAGING_PARENT NEW_DB | apply DB FRAME KEY_FILE STAGING_PARENT".into()),
    }
    Ok(())
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}
