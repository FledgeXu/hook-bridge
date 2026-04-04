.PHONY: test coverage

test:
	cargo test --workspace

coverage:
	cargo llvm-cov --workspace --all-features --lib --tests \
		--ignore-filename-regex 'src/main.rs' \
		--fail-under-lines 95 --summary-only
