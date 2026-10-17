param([string]$OutputDirectory = "")
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$AppRoot = Split-Path -Parent $PSScriptRoot
$RepoRoot = Split-Path -Parent (Split-Path -Parent $AppRoot)
$Version = (Get-Content (Join-Path $AppRoot "package.json") -Raw | ConvertFrom-Json).version
if (!$OutputDirectory) { $OutputDirectory = Join-Path $RepoRoot "artifacts/m-geo-$Version" }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

function Invoke-Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE" }
}

Push-Location $AppRoot
try {
    Invoke-Checked "pnpm" @("tauri", "build", "--ci", "--target", "x86_64-pc-windows-msvc", "--bundles", "nsis", "--", "--locked")
    $TargetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $AppRoot "src-tauri/target" }
    $Release = Join-Path $TargetRoot "x86_64-pc-windows-msvc/release"
    $Installers = @(Get-ChildItem (Join-Path $Release "bundle/nsis") -Filter "*_${Version}_*-setup.exe")
    if ($Installers.Count -ne 1) { throw "Expected exactly one NSIS installer for $Version" }
    $Installer = Join-Path $OutputDirectory "M-GEO-$Version-windows-x64-internal-setup.exe"
    Copy-Item $Installers[0].FullName $Installer
    $SevenZip = Get-Command "7z" -ErrorAction SilentlyContinue
    $SevenZipPath = if ($SevenZip) { $SevenZip.Source } else { Join-Path $env:ProgramFiles "7-Zip/7z.exe" }
    if (!(Test-Path $SevenZipPath)) { throw "Install 7-Zip to inspect the NSIS payload" }
    $Extracted = Join-Path ([IO.Path]::GetTempPath()) ("mgeo-nsis-" + [guid]::NewGuid())
    try {
        Invoke-Checked $SevenZipPath @("x", "-y", "-o$Extracted", $Installer)
        $Report = & python (Join-Path $PSScriptRoot "verify-windows-package.py") --app (Join-Path $Release "m-geo.exe") --installer $Installer --icon (Join-Path $AppRoot "src-tauri/icons/icon.ico") --version $Version --extracted $Extracted --resources (Join-Path $AppRoot "src-tauri/resources")
        if ($LASTEXITCODE -ne 0) { throw "Static Windows package verification failed" }
        $Report | Set-Content -Encoding utf8 (Join-Path $OutputDirectory "windows-package-verification.json")
        $Report | Write-Output
    } finally {
        if (Test-Path $Extracted) { Remove-Item -Recurse -Force $Extracted }
    }
    $Hash = (Get-FileHash $Installer -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $([IO.Path]::GetFileName($Installer))" | Set-Content -Encoding ascii (Join-Path $OutputDirectory "WINDOWS-SHA256SUMS")
    Write-Output "Unsigned Windows x64 package built and statically inspected. Installation and interactive runtime require separate acceptance."
} finally {
    Pop-Location
}
