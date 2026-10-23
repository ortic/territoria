# Builds a Windows installer of Territoria: target\Territoria-<version>-x64.msi
# Needs Rust (MSVC toolchain) and the WiX tool, version 4 or newer:
#   dotnet tool install --global wix
# Run from anywhere: powershell -ExecutionPolicy Bypass -File packaging\build-msi.ps1
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value

cargo build --release -p territoria-app
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    throw 'The WiX tool is missing: dotnet tool install --global wix'
}

$out = "target\Territoria-$version-x64.msi"
wix build packaging\territoria.wxs -arch x64 `
    -d "Version=$version" `
    -d "Root=$root" `
    -d "Packaging=$root\packaging" `
    -d "Target=$root\target\release" `
    -o $out
if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }
Write-Output $out
