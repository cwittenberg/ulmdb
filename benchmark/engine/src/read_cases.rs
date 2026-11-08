fn read_cases(
    data: &Dataset,
    ulmdb: &Environment,
    lmdb: &NativeEnvironment,
    handles: &Handles<'_>,
    measure: bool,
    raw: &mut Samples,
) -> Result<(Vec<Case>, String, String)> {
    // Exactly one default-thread-slot LMDB read transaction spans all bound DB views.
    // All views/readers drop before durable writes begin.
    let u = ulmdb.begin_read();
    let l = lmdb.begin_read()?;
    let uk = u.kv(b"kv")?;
    let lk = l.database(&handles.kv)?;
    let u4 = u.prefix_index(b"ipv4")?;
    let u6 = u.prefix_index(b"ipv6")?;
    let l4 = native::Prefix4::bind(&l, handles, data)?;
    let l6 = native::Prefix6::bind(&l, handles, data)?;
    let mut cases = Vec::new();
    for (query, expected) in data.kv_queries.values.iter().zip(&data.kv_queries.expected) {
        let a = uk.get(query)?.map(|v| Answer {
            length: 0,
            value: native::decode(v),
        });
        let b = lk.get(query)?.map(|v| Answer {
            length: 0,
            value: native::decode(v),
        });
        if a != *expected || b != *expected {
            return Err("independent KV answer mismatch".into());
        }
    }
    for (&query, expected) in data
        .ipv4_queries
        .values
        .iter()
        .zip(&data.ipv4_queries.expected)
    {
        let a = u4.longest_match(Address::V4(query))?.map(|(p, v)| Answer {
            length: p.length(),
            value: native::decode(v),
        });
        if a != *expected || l4.get(query)? != *expected {
            return Err("independent IPv4 LPM answer mismatch".into());
        }
    }
    for (&query, expected) in data
        .ipv6_queries
        .values
        .iter()
        .zip(&data.ipv6_queries.expected)
    {
        let a = u6.longest_match(Address::V6(query))?.map(|(p, v)| Answer {
            length: p.length(),
            value: native::decode(v),
        });
        if a != *expected || l6.get(query)? != *expected {
            return Err("independent IPv6 LPM answer mismatch".into());
        }
    }
    if measure {
        for (group, indices) in harness::groups(&data.kv_queries.expected) {
            cases.push(harness::case(
                format!("ordered_kv_{group}"),
                &data.kv_queries.values,
                &data.kv_queries.expected,
                indices,
                data.config.trials,
                |key| {
                    uk.get(key).unwrap().map(|v| Answer {
                        length: 0,
                        value: native::decode(v),
                    })
                },
                |key| {
                    lk.get(key).unwrap().map(|v| Answer {
                        length: 0,
                        value: native::decode(v),
                    })
                },
                raw,
            )?);
        }
        for (group, indices) in harness::prefix_groups(&data.ipv4_queries.expected) {
            cases.push(harness::case(
                format!("ipv4_lpm_{group}"),
                &data.ipv4_queries.values,
                &data.ipv4_queries.expected,
                indices,
                data.config.trials,
                |&a| {
                    u4.longest_match(Address::V4(a))
                        .unwrap()
                        .map(|(p, v)| Answer {
                            length: p.length(),
                            value: native::decode(v),
                        })
                },
                |&a| l4.get(a).unwrap(),
                raw,
            )?);
        }
        for (group, indices) in harness::prefix_groups(&data.ipv6_queries.expected) {
            cases.push(harness::case(
                format!("ipv6_lpm_{group}"),
                &data.ipv6_queries.values,
                &data.ipv6_queries.expected,
                indices,
                data.config.trials,
                |&a| {
                    u6.longest_match(Address::V6(a))
                        .unwrap()
                        .map(|(p, v)| Answer {
                            length: p.length(),
                            value: native::decode(v),
                        })
                },
                |&a| l6.get(a).unwrap(),
                raw,
            )?);
        }
    }
    Ok((cases, l4.cover(), l6.cover()))
}
