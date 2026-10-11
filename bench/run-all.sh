#!/usr/bin/env sh
# Runs step A of the 4K60 compositing experiment: every variant at N of 2,
# 4 and 8, 600 frames each, rows to out/A-n<N>-v<V>.ndjson, then the table.
#
#   bench/run-all.sh [--frames 600] [--all-4k] [--backend vulkan|gl|metal|dx12] [--out bench/out]
#
# Ns and variants come from the environment: NS="2 4 8" VARIANTS="1 2 3 4".
set -e
cd "$(dirname "$0")/.."
frames=600
all4k=""
backend=""
out=bench/out
while [ $# -gt 0 ]; do
    case "$1" in
        --frames) frames="$2"; shift 2 ;;
        --all-4k) all4k="--all-4k"; shift ;;
        --backend) backend="--backend $2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        *) echo "unknown argument $1" >&2; exit 2 ;;
    esac
done
cargo build --release -p blitz-bench
mkdir -p "$out"
suffix=""
[ -n "$all4k" ] && suffix="-4k"
for n in ${NS:-2 4 8}; do
    for v in ${VARIANTS:-1 2 3 4}; do
        file="$out/A-n$n-v$v$suffix.ndjson"
        echo "== N=$n variant $v -> $file"
        target/release/bench --n "$n" --variant "$v" --frames "$frames" $all4k $backend > "$file" || echo "run failed: N=$n variant $v" >&2
    done
done
target/release/summarize "$out"
