//! Local frozen-data journeys through the actual mapped engine; no DNS calls.
use std::net::IpAddr;
use ulmdb::Environment;
use ulmdb_ipdon::*;

fn address(text: &str) -> Result<Address> {
    text.parse::<IpAddr>()
        .map(Address::from)
        .map_err(|_| Error::InvalidInput("invalid IP"))
}
fn prefix(text: &str) -> Result<Prefix> {
    let (ip, length) = text
        .split_once('/')
        .ok_or(Error::InvalidInput("CIDR requires /length"))?;
    Prefix::new(
        address(ip)?,
        length
            .parse()
            .map_err(|_| Error::InvalidInput("invalid CIDR length"))?,
    )
    .map_err(|_| Error::InvalidInput("noncanonical or invalid CIDR"))
}
fn ip_text(address: Address) -> IpAddr {
    match address {
        Address::V4(ip) => IpAddr::V4(ip.into()),
        Address::V6(ip) => IpAddr::V6(ip.into()),
    }
}
fn role(text: Option<&String>) -> Result<Role> {
    match text.map(String::as_str).unwrap_or("direct") {
        "direct" => Ok(Role::Direct),
        "ns" => Ok(Role::NameserverAddress),
        "mail" => Ok(Role::MailTarget),
        "ptr" => Ok(Role::Ptr),
        _ => Err(Error::InvalidInput("role must be direct/ns/mail/ptr")),
    }
}
fn show(edges: Edges<'_>) -> Result<()> {
    for edge in edges.take(100) {
        let edge = edge?;
        println!(
            "{}\t{}\tsupports={}\trole={:?}",
            edge.hostname,
            ip_text(edge.address),
            edge.supports,
            edge.role
        );
    }
    Ok(())
}
fn demo(path: &str) -> Result<()> {
    let env = Environment::create(path)?;
    let mut write = env.begin_write()?;
    Projection::initialize(&mut write)?;
    for (key, source, hostname, ip, role) in [
        ("a", 7, "Example.Test.", "192.0.2.1", Role::Direct),
        (
            "a-other-source",
            8,
            "example.test",
            "192.0.2.1",
            Role::Direct,
        ),
        ("aaaa", 7, "example.test", "2001:db8::1", Role::Direct),
        (
            "shared",
            7,
            "shared.example.test",
            "192.0.2.1",
            Role::Direct,
        ),
        (
            "ns-address",
            7,
            "ns1.example.test",
            "192.0.2.53",
            Role::NameserverAddress,
        ),
    ] {
        let source = SourceId(source);
        let observation = Observation {
            id: ObservationId::from_source(source, key.as_bytes()),
            hostname: hostname.into(),
            address: address(ip)?,
            scope: ScopeId(1),
            source,
            role,
            observed_at: 1_700_000_000,
            collected_at: 1_700_000_001,
            vantage: VantageId(1),
            resolver: ResolverId(1),
            ttl: 300,
            dnssec: Dnssec::Unchecked,
            evidence: EvidenceId::from_bytes(key.as_bytes()),
        };
        Projection::upsert(&mut write, &observation)?;
    }
    Projection::put_profile(
        &mut write,
        ScopeId(1),
        prefix("192.0.2.0/24")?,
        b"documentation IPv4",
    )?;
    Projection::put_profile(
        &mut write,
        ScopeId(1),
        prefix("2001:db8::/32")?,
        b"documentation IPv6",
    )?;
    println!("committed frozen demo sequence {}", write.commit()?);
    let read = env.begin_read();
    let view = Projection::view(&read)?;
    println!("hostname -> addresses");
    show(view.addresses(ScopeId(1), Role::Direct, "example.test", None)?)?;
    println!("IP -> hostnames");
    show(view.hostnames(ScopeId(1), Role::Direct, address("192.0.2.1")?, None)?)?;
    println!("nameserver address observations");
    show(view.cidr(
        ScopeId(1),
        Role::NameserverAddress,
        prefix("192.0.2.0/24")?,
        None,
    )?)?;
    println!("reconciled {:?}", view.reconcile()?);
    Ok(())
}
fn execute() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let at = |n: usize| {
        args.get(n).map(String::as_str).ok_or(Error::InvalidInput("usage: ipdon demo|check DIRECTORY; hostname|ip|cidr|profiles DIRECTORY SCOPE QUERY [ROLE]; evidence DIRECTORY SCOPE HOST IP [ROLE]"))
    };
    let command = at(0)?;
    let path = at(1)?;
    if command == "demo" {
        return demo(path);
    }
    let env = Environment::open(path)?;
    let read = env.begin_read();
    let view = Projection::view(&read)?;
    if command == "check" {
        println!("{:?}", view.reconcile()?);
        return Ok(());
    }
    let scope = ScopeId(
        at(2)?
            .parse()
            .map_err(|_| Error::InvalidInput("invalid scope"))?,
    );
    match command {
        "hostname" => show(view.addresses(scope, role(args.get(4))?, at(3)?, None)?),
        "ip" => show(view.hostnames(scope, role(args.get(4))?, address(at(3)?)?, None)?),
        "cidr" => show(view.cidr(scope, role(args.get(4))?, prefix(at(3)?)?, None)?),
        "profiles" => {
            if let Some((prefix, value)) = view.profiles(scope)?.longest_match(address(at(3)?)?)? {
                println!(
                    "{}/{}\t{}",
                    ip_text(prefix.network()),
                    prefix.length(),
                    String::from_utf8_lossy(value)
                );
            } else {
                println!("not found");
            }
            Ok(())
        }
        "evidence" => {
            for observation in view
                .evidence(scope, role(args.get(5))?, at(3)?, address(at(4)?)?, None)?
                .take(100)
            {
                println!("{:?}", observation?);
            }
            Ok(())
        }
        _ => Err(Error::InvalidInput("unknown command")),
    }
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
