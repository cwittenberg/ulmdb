# Completed lookup evidence

Start with [the comparison and admission report](REPORT.md). The full [direct table](LOOKUP_FULL_TABLE.md), [1,308-row CSV](LOOKUP_FULL_TABLE.csv), and [all default-path metrics](DEFAULT_CANONICAL.md) retain every measured operation.

[Qualification](qualification.json) records the deliberately limited admission conclusion. [MANIFEST.json](MANIFEST.json) and [SHA256SUMS](SHA256SUMS) bind the raw evidence archive, source trees, samples and published tables. Verify with `python3 benchmark/lookup-strategies/retain_completed.py --output benchmark/lookup-strategies/completed --verify-only`.
