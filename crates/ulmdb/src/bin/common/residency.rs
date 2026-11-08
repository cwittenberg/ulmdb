//! Shared CLI boundary; no configuration parsing occurs on a lookup path.
use ulmdb::{Error, ResidencyPolicy, Result};

pub fn parse(arguments: &[String]) -> Result<(ResidencyPolicy, &[String])> {
    if arguments.first().map(String::as_str) != Some("--residency") {
        return Ok((ResidencyPolicy::Demand, arguments));
    }
    let value = arguments
        .get(1)
        .ok_or(Error::InvalidInput("--residency requires a policy"))?;
    let policy = if value == "demand" {
        ResidencyPolicy::Demand
    } else {
        let (mode, bytes) = value.split_once(':').ok_or(Error::InvalidInput(
            "residency must be demand, prefault:BYTES or locked:BYTES",
        ))?;
        if bytes.is_empty() || !bytes.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Error::InvalidInput(
                "residency budget must be an unsigned byte count",
            ));
        }
        let max_bytes = bytes
            .parse()
            .map_err(|_| Error::InvalidInput("residency budget exceeds u64"))?;
        match mode {
            "prefault" => ResidencyPolicy::Prefault { max_bytes },
            "locked" | "required_locked" => ResidencyPolicy::RequiredLocked { max_bytes },
            _ => return Err(Error::InvalidInput("unknown residency policy")),
        }
    };
    Ok((policy, &arguments[2..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_policies_preserve_commands_and_reject_ambiguous_budgets() {
        let input = ["get", "db", "table", "key"].map(String::from);
        assert_eq!(
            parse(&input).unwrap(),
            (ResidencyPolicy::Demand, input.as_slice())
        );
        for (text, expected) in [
            ("demand", ResidencyPolicy::Demand),
            ("prefault:0", ResidencyPolicy::Prefault { max_bytes: 0 }),
            (
                "locked:65536",
                ResidencyPolicy::RequiredLocked { max_bytes: 65536 },
            ),
            (
                "required_locked:18446744073709551615",
                ResidencyPolicy::RequiredLocked {
                    max_bytes: u64::MAX,
                },
            ),
        ] {
            let input = ["--residency", text, "get", "db"].map(String::from);
            let (policy, commands) = parse(&input).unwrap();
            assert_eq!(policy, expected);
            assert_eq!(commands, ["get", "db"]);
        }
        for text in [
            "prefault",
            "locked",
            "locked:",
            "locked:-1",
            "locked:+1",
            "locked:1_000",
            "locked: 1",
            "locked:18446744073709551616",
            "demand:1",
            "other:1",
        ] {
            assert!(
                parse(&["--residency".into(), text.into()]).is_err(),
                "{text}"
            );
        }
        assert!(parse(&["--residency".into()]).is_err());
    }
}
