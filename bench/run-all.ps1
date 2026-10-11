# Runs step A of the 4K60 compositing experiment: every variant at N of 2,
# 4 and 8, 600 frames each, rows to out/A-n<N>-v<V>.ndjson, then the table.
#
#   .\bench\run-all.ps1 [-Frames 600] [-All4k] [-Backend dx12|vulkan|gl|metal] [-Ns 2,4,8] [-Variants 1,2,3,4] [-Out bench/out]
#
# The bench is built in release first (cargo build --release -p blitz-bench).
param(
    [int]$Frames = 600,
    [switch]$All4k,
    [string]$Backend = "",
    [int[]]$Ns = @(2, 4, 8),
    [int[]]$Variants = @(1, 2, 3, 4),
    [string]$Out = "bench/out"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $root
try {
    cargo build --release -p blitz-bench
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
    New-Item -ItemType Directory -Force $Out | Out-Null
    $suffix = if ($All4k) { "-4k" } else { "" }
    foreach ($n in $Ns) {
        foreach ($v in $Variants) {
            $file = Join-Path $Out "A-n$n-v$v$suffix.ndjson"
            $args = @("--n", $n, "--variant", $v, "--frames", $Frames)
            if ($All4k) { $args += "--all-4k" }
            if ($Backend) { $args += @("--backend", $Backend) }
            Write-Host "== N=$n variant $v -> $file"
            & target/release/bench.exe @args > $file
            if ($LASTEXITCODE -ne 0) { Write-Warning "run failed: N=$n variant $v" }
        }
    }
    & target/release/summarize.exe $Out
} finally {
    Pop-Location
}
