#!/usr/bin/env bash
failed=0
for file in $(find src tests -name '*.rs'); do
    lines=$(wc -l <"$file")
    if [ "$lines" -gt 800 ]; then
        echo "File too long: $file ($lines lines)"
        failed=1
    fi
done
exit "$failed"
