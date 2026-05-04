param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot

Write-Host "=== VRCRecoder Spout2 SDK Setup ==="

# Clone Spout2 SDK if not vendored
$spoutDir = Join-Path $ProjectRoot "vendor\Spout2"
if (-not (Test-Path $spoutDir)) {
    Write-Host "Cloning Spout2 SDK..."
    Push-Location (Join-Path $ProjectRoot "vendor")
    git submodule add https://github.com/leadedge/Spout2 Spout2 2>&1 | Out-Null
    git submodule update --init --recursive
    Pop-Location
} else {
    Write-Host "Spout2 SDK already vendored at $spoutDir"
}

if ($SkipBuild) {
    Write-Host "Skipping build (--SkipBuild specified)"
    exit 0
}

# Build Spout2 with CMake
$buildDir = Join-Path $spoutDir "cmake-build"
if (-not (Test-Path $buildDir)) {
    New-Item -ItemType Directory -Path $buildDir | Out-Null
}

Push-Location $buildDir
Write-Host "Running CMake configure..."
cmake .. -G "Visual Studio 17 2022" -A x64
Write-Host "Running CMake build (Release)..."
cmake --build . --config Release
Pop-Location

# Verify outputs
$libPath = Join-Path $buildDir "lib\Release\Spout.lib"
$dllPath = Join-Path $buildDir "bin\Release\Spout.dll"

$ok = $true
if (Test-Path $libPath) {
    Write-Host "[OK] Spout.lib: $libPath"
} else {
    Write-Host "[FAIL] Spout.lib not found at $libPath"
    $ok = $false
}
if (Test-Path $dllPath) {
    Write-Host "[OK] Spout.dll: $dllPath"
} else {
    Write-Host "[FAIL] Spout.dll not found at $dllPath"
    $ok = $false
}

if ($ok) {
    Write-Host ""
    Write-Host "Setup complete. Copy Spout.dll next to your built .exe before running:"
    Write-Host "  copy $dllPath target\release\"
} else {
    Write-Host ""
    Write-Host "Build failed. Check CMake output above."
    Write-Host "Ensure Visual Studio 2022 is installed with C++ desktop workload."
    exit 1
}
