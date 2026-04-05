#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
script_dir="$repo_root/scripts/hooks"

cd "$repo_root"
exec python3 "$script_dir/auto_review.py"
