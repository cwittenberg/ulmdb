use std::collections::BTreeSet;
pub fn assert_random(bytes: &[u8], expected: bool) {
    assert!(!bytes.is_empty());
    let address = bytes.as_ptr() as usize;
    let mut header = String::new();
    let mut active = false;
    for line in std::fs::read_to_string("/proc/self/smaps").unwrap().lines() {
        if let Some((start, end)) = line
            .split_whitespace()
            .next()
            .and_then(|s| s.split_once('-'))
            && let (Ok(start), Ok(end)) = (
                usize::from_str_radix(start, 16),
                usize::from_str_radix(end, 16),
            )
        {
            active = start <= address && address < end;
            if active {
                header = line.to_owned();
            }
        }
        if active && let Some(flags) = line.strip_prefix("VmFlags:") {
            assert!(
                header.contains(".seg") || header.contains(".cohort"),
                "borrow is not backed by a genuine segment: {header}"
            );
            let flags = flags.split_whitespace().collect::<BTreeSet<_>>();
            assert_eq!(
                flags.contains("rr"),
                expected,
                "wrong advice for {header}: {flags:?}"
            );
            return;
        }
    }
    panic!("borrowed value mapping not found");
}
