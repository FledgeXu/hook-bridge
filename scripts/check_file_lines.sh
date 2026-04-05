#!/usr/bin/env bash
failed=0
while IFS= read -r -d '' file; do
    lines=$(wc -l <"$file")
    if [ "$lines" -gt 800 ]; then
        echo "File too long: $file ($lines lines)"
        failed=1
    fi
done < <(find src tests -name '*.rs' -print0)
exit "$failed"
