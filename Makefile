.PHONY: test coverage coverage-gate

test:
	cargo test --workspace

coverage:
	cargo llvm-cov --workspace --all-features --lib --tests \
		--fail-under-lines 95 --summary-only

coverage-gate: coverage
