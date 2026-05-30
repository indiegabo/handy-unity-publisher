# PowerShell script to set up docs virtualenv and install dependencies.
# Usage: .\scripts\setup-docs.ps1

$repoRoot = Split-Path -Parent $PSScriptRoot
$venvPath = Join-Path $repoRoot "tmp\docs-venv"
Write-Host "Creating virtual environment at $venvPath"
python -m venv $venvPath
$pythonExe = Join-Path $venvPath "Scripts\python.exe"
& $pythonExe -m pip install --upgrade pip
& $pythonExe -m pip install -r (Join-Path $repoRoot "requirements.txt")
Write-Host "Done. To preview docs run:"
Write-Host "`t$($pythonExe) -m mkdocs serve"
