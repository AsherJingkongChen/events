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

# The five judgments that are ours: when an event runs, which reply settles it, how long the loop
# sleeps, that a second serve cannot hold the home, and that a line of .env is never told.
test:
  cargo test
