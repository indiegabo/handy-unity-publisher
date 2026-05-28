param(
    [Parameter(Mandatory = $true)]
    [string]$TargetTriple,

    [Parameter()]
    [string]$BuildProfile = "release"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if ($cargoCommand) {
    $cargoPath = $cargoCommand.Source
}
else {
    $cargoPath = Join-Path $env:USERPROFILE ".cargo/bin/cargo.exe"
}

if (-not (Test-Path $cargoPath)) {
    throw "cargo executable was not found; expected at $cargoPath"
}

$targetDirectory = & $cargoPath metadata --format-version 1 --no-deps |
ConvertFrom-Json |
Select-Object -ExpandProperty target_directory

if (-not $targetDirectory) {
    throw "cargo metadata did not report a target directory"
}

$profileArgs = @()
$profileDirectory = $BuildProfile

if ($BuildProfile -eq "release") {
    $profileArgs += "--release"
    $profileDirectory = "release"
}
else {
    $profileArgs += @("--profile", $BuildProfile)
}

$cargoArgs = @(
    "build",
    "--locked",
    "--package",
    "runtime-bin",
    "--target",
    $TargetTriple
) + $profileArgs

& $cargoPath @cargoArgs
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$extension = if ($TargetTriple.Contains("windows")) { ".exe" } else { "" }
$sourcePath = Join-Path $targetDirectory "$TargetTriple/$profileDirectory/hgp-runtime$extension"

if (-not (Test-Path $sourcePath)) {
    throw "built runtime binary was not found at $sourcePath"
}

$destinationDir = Join-Path $repoRoot "src-tauri/bin"
New-Item -Path $destinationDir -ItemType Directory -Force | Out-Null

$destinationPath = Join-Path $destinationDir "hgp-runtime-$TargetTriple$extension"
Copy-Item -Path $sourcePath -Destination $destinationPath -Force

Write-Host "Prepared Tauri runtime sidecar at $destinationPath"