.PHONY: test mutants coverage verify

test:
	cargo nextest run --workspace --all-features

mutants:
	cargo mutants --test-tool=nextest --jobs 4

coverage:
	cargo llvm-cov --workspace --all-features --lib --tests --fail-under-lines 95 --summary-only

verify:
	cargo fmt --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo nextest run --workspace --all-features
	cargo llvm-cov --workspace --all-features --lib --tests --fail-under-lines 95 --summary-only
	cargo mutants --test-tool=nextest --jobs 4
