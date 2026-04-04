.PHONY: test coverage coverage-gate verify

test:
	cargo test --workspace

coverage:
	cargo llvm-cov --workspace --all-features --lib --tests \
		--fail-under-lines 95 --summary-only

coverage-gate: coverage

verify:
	cargo fmt --check
	cargo clippy --all-targets --all-features -- -D warnings
	$(MAKE) coverage-gate
