.PHONY: test mutants coverage check-lines verify fmt clippy

test:
	cargo nextest run --workspace --all-features

mutants:
	cargo mutants --test-tool=nextest --jobs 4

coverage:
	cargo llvm-cov --workspace --all-features --lib --tests --fail-under-lines 95 --summary-only

check-lines:
	./scripts/check_file_lines.sh

fmt:
	cargo fmt --check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

verify:
	cargo fmt --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo nextest run --workspace --all-features
	cargo llvm-cov --workspace --all-features --lib --tests --fail-under-lines 95 --summary-only
	cargo mutants --test-tool=nextest --jobs 4
	./scripts/check_file_lines.sh
