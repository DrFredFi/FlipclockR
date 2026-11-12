<#
.SYNOPSIS
Runs the tests, builds a release Flipclock and copies it into a folder ready to use.

.DESCRIPTION
The output folder gets flipclock.exe. An existing config.toml there is always
kept, so publishing over an installed copy keeps its settings.

.EXAMPLE
.\publish.ps1                                 # tests, then publishes to .\publish
.\publish.ps1 -Output D:\Tools\Flipclock      # anywhere else
.\publish.ps1 -Platform ARM64 -Clean          # Windows on Arm, emptying the folder first
#>
param(
    [string]$Output = (Join-Path $PSScriptRoot 'publish'),
    [ValidateSet('x64', 'ARM64')]
    [string]$Platform = 'x64',
    # Empty the output folder first (config.toml is still kept).
    [switch]$Clean,
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$target = @{ x64 = 'x86_64-pc-windows-msvc'; ARM64 = 'aarch64-pc-windows-msvc' }[$Platform]

function Invoke-Cargo {
    & cargo @args
    if ($LASTEXITCODE -ne 0) { throw "cargo $($args -join ' ') failed (exit code $LASTEXITCODE)" }
}

if (-not $SkipTests) {
    Write-Host '==> Running the tests' -ForegroundColor Cyan
    Invoke-Cargo test --workspace
}

Write-Host "==> Building the release for $Platform" -ForegroundColor Cyan
rustup target add $target | Out-Null
Invoke-Cargo build --release -p flipclock-app --target $target

$exe = Join-Path $PSScriptRoot "target\$target\release\flipclock.exe"
New-Item -ItemType Directory -Force $Output | Out-Null
if ($Clean) {
    Get-ChildItem $Output -Force | Where-Object Name -ne 'config.toml' | Remove-Item -Recurse -Force
}

try {
    Copy-Item $exe $Output -Force
} catch {
    throw "Cannot replace $(Join-Path $Output 'flipclock.exe'): close Flipclock if it is running from there, then try again."
}

Write-Host "==> Published to $((Resolve-Path $Output).Path)" -ForegroundColor Green
if (Test-Path (Join-Path $Output 'config.toml')) {
    Write-Host '    (existing config.toml kept)'
}
