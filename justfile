set positional-arguments

# default recipe to display help information
default:
  @just --list

# Fixes the formatting of the workspace
fmt-fix:
  cargo +nightly fmt --all

# Check the formatting of the workspace
fmt-check:
  cargo +nightly fmt --all -- --check

# Lint the workspace
lint: fmt-check
  cargo +nightly clippy --workspace --all --all-features --all-targets -- -D warnings

# Lint the Rust documentation
lint-docs:
  RUSTDOCFLAGS="-D warnings" cargo doc --all --no-deps --document-private-items

# Test the Rust documentation
test-docs:
  cargo test --doc --all --locked

# Build the workspace
build *args='':
  cargo build --workspace --all $@

# Run FPP tests
test *args='':
  cargo nextest run --workspace --all $@

# Runs `cargo hack check` against the workspace
hack *args='':
  cargo hack --feature-powerset --no-dev-deps $@
