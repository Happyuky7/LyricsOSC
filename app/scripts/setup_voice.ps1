# LyricsOSC - voice engine setup (Windows). Run by the installer; safe to re-run.
# Installs a private Python + faster-whisper inside the install folder.
# If an NVIDIA GPU is detected, CUDA libraries are installed automatically.
param(
    [string]$InstallDir = (Split-Path -Parent $PSScriptRoot),
    [string]$Model = "small",
    [switch]$NoGpu
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
[Console]::OutputEncoding = [Text.Encoding]::UTF8

$py     = Join-Path $InstallDir "python"
$venv   = Join-Path $py ".venv"
$tools  = Join-Path $InstallDir "tools"
$uv     = Join-Path $tools "uv.exe"
$env:UV_PYTHON_INSTALL_DIR = Join-Path $py "runtime"   # keep Python inside the install folder
$env:UV_CACHE_DIR          = Join-Path $py "uv-cache"
$env:HF_HOME               = Join-Path $InstallDir "models"  # Whisper models

function Step($m) { Write-Host "`n==> $m" -ForegroundColor Magenta }

New-Item -ItemType Directory -Force -Path $tools, $py | Out-Null

if (-not (Test-Path $uv)) {
    Step "Downloading uv (Python manager)"
    $zip = Join-Path $env:TEMP "uv-lyricsosc.zip"
    Invoke-WebRequest "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-pc-windows-msvc.zip" -OutFile $zip
    Expand-Archive $zip -DestinationPath $tools -Force
    Remove-Item $zip
}

if (-not (Test-Path (Join-Path $venv "Scripts\python.exe"))) {
    Step "Creating Python 3.12 environment"
    & $uv venv --python 3.12 $venv
    if ($LASTEXITCODE) { throw "uv venv failed" }
}

Step "Installing faster-whisper"
& $uv pip install --python (Join-Path $venv "Scripts\python.exe") -r (Join-Path $py "requirements.txt")
if ($LASTEXITCODE) { throw "pip install failed" }

$gpu = $null
if (-not $NoGpu) {
    try { $gpu = (& nvidia-smi --query-gpu=name --format=csv,noheader 2>$null | Select-Object -First 1) } catch {}
}
if ($gpu) {
    Step "NVIDIA GPU detected ($gpu): installing CUDA libraries"
    & $uv pip install --python (Join-Path $venv "Scripts\python.exe") nvidia-cublas-cu12 "nvidia-cudnn-cu12==9.*"
    if ($LASTEXITCODE) { Write-Warning "CUDA libraries failed to install; the CPU will be used." }
} else {
    Step "No NVIDIA GPU detected: Whisper will use the CPU"
}

Step "Downloading Whisper model '$Model'"
& (Join-Path $venv "Scripts\python.exe") -c "from faster_whisper.utils import download_model; download_model('$Model')"

Step "Done"
