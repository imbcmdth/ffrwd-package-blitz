# Runs step C of the 4K60 compositing experiment: the harness as a wasm source
# node under ffrwd-wasm, through wgpu-wasi-webgpu, every variant at N of 2, 4
# and 8, rows to out/C-n<N>-v<V>.ndjson, then the table.
#
#   .\bench\run-c.ps1 [-Frames 600] [-All4k] [-Ns 2,4,8] [-Variants 1,2,3,4] [-Out bench/out]
#                     [-FfrwdWasm ..\ffrwd-cli\sidecar\target\release\ffrwd-wasm.exe]
#
# The module is built first (cargo build -p blitz-bench-compose --target
# wasm32-wasip2 --release). The readback variant maps the picture to a null
# output so the frame leaves the module as it would in a query; the others
# leave the picture unmapped, so no readback is forced on them.
param(
    [int]$Frames = 600,
    [switch]$All4k,
    [int[]]$Ns = @(2, 4, 8),
    [int[]]$Variants = @(1, 2, 3, 4),
    [string]$Out = "bench/out",
    [string]$FfrwdWasm = "..\ffrwd-cli\sidecar\target\release\ffrwd-wasm.exe"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $root
try {
    cargo build -p blitz-bench-compose --target wasm32-wasip2 --release
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
    $module = (Resolve-Path "target/wasm32-wasip2/release/blitz_bench_compose.wasm").Path
    $ffrwd = (Resolve-Path $FfrwdWasm).Path
    New-Item -ItemType Directory -Force $Out | Out-Null
    $suffix = if ($All4k) { "-4k" } else { "" }
    $all4kParam = if ($All4k) { "true" } else { "false" }
    foreach ($n in $Ns) {
        foreach ($v in $Variants) {
            $file = Join-Path $Out "C-n$n-v$v$suffix.ndjson"
            $params = "inputs=${n}:variant=${v}:all_4k=${all4kParam}:frames=${Frames}"
            if ($v -eq 3) {
                $chain = "bench=$params[video=pic][@rows=r]"
                $args = @("-gpu", $module, "-m", "bench=$module", "-filter_complex", $chain,
                          "-map", "[pic]", "-f", "null", "-", "-map", "[r]", "-f", "ndjson", $file)
            } else {
                $chain = "bench=$params[@rows=r]"
                $args = @("-gpu", $module, "-m", "bench=$module", "-filter_complex", $chain,
                          "-map", "[r]", "-f", "ndjson", $file)
            }
            Write-Host "== N=$n variant $v -> $file"
            & $ffrwd @args
            if ($LASTEXITCODE -ne 0) { Write-Warning "run failed: N=$n variant $v" }
        }
    }
    cargo build --release -p blitz-bench --bin summarize
    & target/release/summarize.exe $Out
} finally {
    Pop-Location
}
