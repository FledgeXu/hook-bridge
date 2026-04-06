#!/usr/bin/env bash

file="Cargo.toml"

if git diff --name-only HEAD -- | grep -qF "$file"; then
    printf '%s modified\n' "$file"
    exit 1
fi
