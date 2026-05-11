param(
    [Parameter(Mandatory = $true)]
    [string]$TargetTriple,

    [Parameter()]
    [string]$BuildProfile = "release"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

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

& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$extension = if ($TargetTriple.Contains("windows")) { ".exe" } else { "" }
$sourcePath = Join-Path $repoRoot "target/$TargetTriple/$profileDirectory/runtime-bin$extension"

if (-not (Test-Path $sourcePath)) {
    throw "built runtime binary was not found at $sourcePath"
}

$destinationDir = Join-Path $repoRoot "apps/desktop/src-tauri/bin"
New-Item -Path $destinationDir -ItemType Directory -Force | Out-Null

$destinationPath = Join-Path $destinationDir "runtime-bin-$TargetTriple$extension"
Copy-Item -Path $sourcePath -Destination $destinationPath -Force

Write-Host "Prepared Tauri runtime sidecar at $destinationPath"