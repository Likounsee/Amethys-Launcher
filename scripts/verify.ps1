# Runs the complete local verification sequence and prints a compact report.
$ErrorActionPreference = "Continue"
$checks = [System.Collections.Generic.List[object]]::new()

function Invoke-Check {
    param(
        [string]$Name,
        [string]$Executable,
        [string[]]$Arguments
    )

    try {
        $output = @(& $Executable @Arguments 2>&1)
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 1 }
        $details = ($output | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
    }
    catch {
        $exitCode = 1
        $details = $_.Exception.Message
    }

    $checks.Add([PSCustomObject]@{
        Name = $Name
        ExitCode = $exitCode
        Details = $details
    }) | Out-Null
}

Invoke-Check "Git pull" "git" @("pull")

if ($checks[0].ExitCode -eq 0) {
    Invoke-Check "Format" "cargo" @("fmt", "--all", "--", "--check")
    Invoke-Check "Clippy" "cargo" @("clippy", "--workspace", "--all-targets", "--", "-D", "warnings")
    Invoke-Check "Build" "cargo" @("build", "--workspace")
    Invoke-Check "Tests" "cargo" @("test", "--workspace", "--all-targets")
}

Write-Host ""
Write-Host "=== AMETHYS VERIFICATION ===" -ForegroundColor Cyan

$names = @("Git pull", "Format", "Clippy", "Build", "Tests")
foreach ($name in $names) {
    $check = $checks | Where-Object { $_.Name -eq $name } | Select-Object -First 1
    if ($null -eq $check) {
        Write-Host ("{0,-10} SKIP" -f $name) -ForegroundColor Yellow
    }
    elseif ($check.ExitCode -eq 0) {
        Write-Host ("{0,-10} OK" -f $name) -ForegroundColor Green
    }
    else {
        Write-Host ("{0,-10} ERREUR (code {1})" -f $name, $check.ExitCode) -ForegroundColor Red
    }
}

$failed = @($checks | Where-Object { $_.ExitCode -ne 0 }).Count
$passed = @($checks | Where-Object { $_.ExitCode -eq 0 }).Count
$skipped = 5 - $checks.Count

Write-Host ""
Write-Host ("Réussies : {0}/5 | Erreurs : {1} | Non lancées : {2}" -f $passed, $failed, $skipped)

foreach ($check in ($checks | Where-Object { $_.ExitCode -ne 0 })) {
    Write-Host ""
    Write-Host ("--- Détails : {0} ---" -f $check.Name) -ForegroundColor Red
    $lines = @($check.Details -split "`r?`n" | Where-Object { $_ -match "(?i)error|warning|failed|could not|cannot|denied|panicked|not found|access is denied" })
    if ($lines.Count -eq 0) {
        $lines = @($check.Details -split "`r?`n" | Select-Object -Last 12)
    }
    $lines | Select-Object -Last 20 | ForEach-Object { Write-Host $_ }
}

if ($failed -eq 0 -and $skipped -eq 0) {
    Write-Host "SUCCÈS : 0 ERREUR" -ForegroundColor Green
    exit 0
}

Write-Host "ÉCHEC OU VÉRIFICATIONS INCOMPLÈTES" -ForegroundColor Red
exit 1
