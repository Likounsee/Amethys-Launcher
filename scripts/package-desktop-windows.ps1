$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$GhostEngineRoot = Join-Path (Split-Path -Parent $RepoRoot) "GhostEngine"
$Manifest = Join-Path $RepoRoot "prototypes/ghostengine-amethys-preview/Cargo.toml"
$TargetDir = Join-Path $RepoRoot "target"
$StageDir = Join-Path $RepoRoot "dist/Amethys"
$Archive = Join-Path $RepoRoot "dist/Amethys-Windows-x64.zip"

if (-not (Test-Path (Join-Path $GhostEngineRoot "ghostengine/crates/ghostengine/Cargo.toml"))) {
    throw "GhostEngine sibling checkout not found at $GhostEngineRoot"
}

$env:CARGO_TARGET_DIR = $TargetDir
Push-Location $RepoRoot
try {
    cargo build --release -p amethys-agent
    if ($LASTEXITCODE -ne 0) { throw "Building amethys-agent failed." }

    cargo build --release --manifest-path $Manifest --bin amethys-launcher
    if ($LASTEXITCODE -ne 0) { throw "Building the GhostEngine desktop application failed." }

    if (Test-Path $StageDir) {
        Remove-Item -LiteralPath $StageDir -Recurse -Force
    }
    New-Item -ItemType Directory -Force $StageDir | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $StageDir "assets") | Out-Null

    Copy-Item (Join-Path $TargetDir "release/amethys-agent.exe") $StageDir
    Copy-Item (Join-Path $TargetDir "release/amethys-launcher.exe") $StageDir
    Copy-Item (Join-Path $RepoRoot "prototypes/ghostengine-amethys-preview/assets/fonts") (Join-Path $StageDir "assets") -Recurse
    Copy-Item (Join-Path $RepoRoot "prototypes/ghostengine-amethys-preview/assets/covers") (Join-Path $StageDir "assets") -Recurse
    Copy-Item (Join-Path $RepoRoot "README.md") $StageDir
    Copy-Item (Join-Path $RepoRoot "LICENSE") $StageDir

    @'
Amethys Launcher — Windows x64 (development build)

Run amethys-launcher.exe. It starts amethys-agent.exe automatically when the agent is not already running.
Your local Amethys database and configuration remain in the normal per-user Amethys data directory.
This is a portable development package, not an MSI installer or a production release.
'@ | Set-Content -Encoding UTF8 (Join-Path $StageDir "LIRE-MOI.txt")

    $DistDir = Split-Path -Parent $Archive
    New-Item -ItemType Directory -Force $DistDir | Out-Null
    if (Test-Path $Archive) { Remove-Item -LiteralPath $Archive -Force }
    Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $Archive -CompressionLevel Optimal
    Write-Host "Package created: $Archive"
}
finally {
    Pop-Location
}
