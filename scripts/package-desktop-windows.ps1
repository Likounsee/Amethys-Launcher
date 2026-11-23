$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Manifest = Join-Path $RepoRoot "prototypes/ghostengine-amethys-preview/Cargo.toml"
$TargetDir = Join-Path $RepoRoot "target"
$StageDir = Join-Path $RepoRoot "dist/Amethys"
$Archive = Join-Path $RepoRoot "dist/Amethys-Windows-x64.zip"

if (-not (Test-Path (Join-Path $RepoRoot "vendor/ghostengine/Cargo.toml"))) {
    throw "Vendored GhostEngine source is missing from this Amethys-Launcher checkout."
}

$env:CARGO_TARGET_DIR = $TargetDir
Push-Location $RepoRoot
try {
    # Packaging is a release-like operation: refuse to produce an archive from
    # a tree that fails formatting, linting or tests.
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw "Workspace Rust formatting check failed." }

    cargo fmt --manifest-path $Manifest -- --check
    if ($LASTEXITCODE -ne 0) { throw "Desktop Rust formatting check failed." }

    cargo clippy --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "Workspace Clippy checks failed." }

    cargo clippy --manifest-path $Manifest --bin amethys-launcher -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "Desktop Clippy checks failed." }

    cargo test --workspace --all-targets
    if ($LASTEXITCODE -ne 0) { throw "Workspace tests failed." }

    cargo test --manifest-path $Manifest --bin amethys-launcher
    if ($LASTEXITCODE -ne 0) { throw "Desktop launcher tests failed." }

    cargo build --release -p amethys-agent
    if ($LASTEXITCODE -ne 0) { throw "Building amethys-agent failed." }

    cargo build --release -p amethys-cli --bin amethysctl
    if ($LASTEXITCODE -ne 0) { throw "Building amethysctl failed." }

    cargo build --release --manifest-path $Manifest --bin amethys-launcher
    if ($LASTEXITCODE -ne 0) { throw "Building the GhostEngine desktop application failed." }

    if (Test-Path $StageDir) {
        Remove-Item -LiteralPath $StageDir -Recurse -Force
    }
    New-Item -ItemType Directory -Force $StageDir | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $StageDir "assets") | Out-Null

    Copy-Item (Join-Path $TargetDir "release/amethys-agent.exe") $StageDir
    Copy-Item (Join-Path $TargetDir "release/amethysctl.exe") $StageDir
    Copy-Item (Join-Path $TargetDir "release/amethys-launcher.exe") $StageDir
    $FontSource = Join-Path $RepoRoot "prototypes/ghostengine-amethys-preview/assets/fonts"
    $CoverSource = Join-Path $RepoRoot "prototypes/ghostengine-amethys-preview/assets/covers"
    foreach ($RequiredDir in @($FontSource, $CoverSource)) {
        if (-not (Test-Path $RequiredDir -PathType Container)) {
            throw "Required desktop assets are missing: $RequiredDir"
        }
    }
    Copy-Item $FontSource (Join-Path $StageDir "assets") -Recurse
    Copy-Item $CoverSource (Join-Path $StageDir "assets") -Recurse
    Copy-Item (Join-Path $RepoRoot "README.md") $StageDir
    Copy-Item (Join-Path $RepoRoot "LICENSE") $StageDir

    # Include checksums for every staged file so a copied portable package can
    # be verified without relying on the build machine.
    $ManifestLines = Get-ChildItem -LiteralPath $StageDir -File -Recurse |
        Sort-Object FullName |
        ForEach-Object {
            $Relative = $_.FullName.Substring($StageDir.Length + 1).Replace("\", "/")
            $Hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            "$Hash  $Relative"
        }
    [System.IO.File]::WriteAllLines(
        (Join-Path $StageDir "SHA256SUMS.txt"),
        [string[]]$ManifestLines,
        [System.Text.UTF8Encoding]::new($false)
    )

    @'
Amethys Launcher — Windows x64 (development build)

Run amethys-launcher.exe. It starts amethys-agent.exe automatically when the agent is not already running.
Use amethysctl.exe from PowerShell to import and manage games (run amethysctl.exe --help for commands).
Your local Amethys database and configuration remain in the normal per-user Amethys data directory.
This is a portable development package, not an MSI installer or a production release.
'@ | Set-Content -Encoding UTF8 (Join-Path $StageDir "LIRE-MOI.txt")

    $DistDir = Split-Path -Parent $Archive
    New-Item -ItemType Directory -Force $DistDir | Out-Null
    if (Test-Path $Archive) { Remove-Item -LiteralPath $Archive -Force }
    Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $Archive -CompressionLevel Optimal
    $ArchiveHash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText(
        "$Archive.sha256",
        "$ArchiveHash  $(Split-Path -Leaf $Archive)" + [Environment]::NewLine,
        [System.Text.UTF8Encoding]::new($false)
    )
    Write-Host "Package created: $Archive"
    Write-Host "SHA-256: $ArchiveHash"
}
finally {
    Pop-Location
}
