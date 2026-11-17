set shell := ['/usr/bin/env', 'bash', '-cueo', 'pipefail']

# Everything CI runs.
all: check test

# Check the formatting, and lint: warnings are errors, and Cargo.toml denies or forbids the rest.
check:
  cargo fmt --check
  cargo clippy --all-targets -- --deny warnings

# Fix the formatting, then check.
check-fix:
  cargo fmt
  just check

# The four judgments that are ours: when an event runs, that the command holds nothing private,
# that curl's config lines keep a value whole, and that a second serve cannot hold the home.
test:
  cargo test
