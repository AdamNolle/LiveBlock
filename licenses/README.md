# Desktop dependency licensing evidence

LiveBlock's production desktop packages are inference-only. Dependency policy is
generated from the three locked Cargo graphs and the locked npm graph.

## Explicit OR choices

`dependency-license-decisions.json` binds an exact Package URL and declared SPDX
expression to the permissive branch selected for distribution. It currently
selects MIT for `r-efi` 5.3.0 and 6.0.0. A decision cannot override unknown or
restricted terms, cannot select an undeclared branch, and fails if its component
or expression changes.

## MPL source availability

`mpl-source-offer.json` covers every component still reported as requiring MPL
obligation review. Each entry binds the exact crate/version, Cargo.lock archive
checksum, canonical crates.io source archive, unmodified status, and the pinned
MPL-2.0 license text in `MPL-2.0.txt`.

The source archives are offered through their immutable versioned crates.io
URLs. LiveBlock does not modify the listed crate sources. This bundle prepares
source-availability and notice evidence; it is not legal advice or a human legal
approval. Production distribution remains blocked until an accountable release
operator reviews the generated report and ensures these files travel with the
chosen package/update channel.

CI runs `tools/verify_dependency_obligations.py` against the generated license
report and all Cargo locks. It rejects missing/extra review items, stale OR
choices, checksum drift, malformed URLs, changed license text, or post-generation
mismatch.
