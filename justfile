set shell := ["bash", "-lc"]

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets --all-features

test:
	cargo test -p trust-runtime --test complete_program
	cargo test --all

check:
	cargo check --all

lint: fmt clippy
