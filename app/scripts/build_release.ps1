# Builds LyricsOSC and the installers that the available tools allow.
#   powershell -ExecutionPolicy Bypass -File scripts\build_release.ps1
# Output in dist\:  windows\ (portable)  *-setup.exe (Inno)  *.msi (WiX)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
Write-Host "LyricsOSC $version" -ForegroundColor Magenta

cargo build --release
if ($LASTEXITCODE) { throw "cargo build failed" }

$dist = Join-Path $root "dist\windows"
# Refresh app files only: keep a voice engine / settings already installed in dist for testing
$keep = @("python", "models", "tools", "cache", "config.json", "lyricsosc.log")
Get-ChildItem $dist -ErrorAction SilentlyContinue | Where-Object { $_.Name -notin $keep } | Remove-Item -Recurse -Force
New-Item -ItemType Directory -Force "$dist\python", "$dist\scripts", "$dist\assets" | Out-Null
Copy-Item target\release\lyricsosc.exe "$dist\LyricsOSC.exe"
Copy-Item python\whisper_sidecar.py, python\requirements.txt "$dist\python\"
Copy-Item scripts\setup_voice.ps1 "$dist\scripts\"
# Runtime artwork only (the high-resolution originals in assets\source stay in the repo)
Copy-Item assets\banner.png, assets\logo.png, assets\icon.png, assets\icon.ico, assets\LICENSE-ART.md "$dist\assets\"
Copy-Item ..\LICENSE, ..\README.md, ..\THIRD_PARTY_NOTICES.md "$dist\"
if (Test-Path AxisOpenApi.dll) { Copy-Item AxisOpenApi.dll "$dist\" }   # Axis addon mode
# Portable zip: app files only (no downloaded voice engine, models or user settings)
$stage = Join-Path $root "dist\_zip"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage\python" | Out-Null
Get-ChildItem $dist | Where-Object { $_.Name -notin $keep } | Copy-Item -Destination $stage -Recurse
Copy-Item "$dist\python\whisper_sidecar.py", "$dist\python\requirements.txt" "$stage\python\"
Compress-Archive -Force "$stage\*" "dist\LyricsOSC-$version-windows-portable.zip"
# (the staged folder is also the source of the .msi below)

$iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if ($iscc) { & $iscc "/DAppVersion=$version" installer\inno\lyricsosc.iss } else { Write-Warning "Inno Setup 6 not found: skipping .exe installer (winget install JRSoftware.InnoSetup)" }

if (Get-Command wix -ErrorAction SilentlyContinue) {
    # Same app-only files as the portable zip, passed as a bind path (see lyricsosc.wxs)
    $msi = "dist\LyricsOSC-$version.msi"
    wix build installer\wix\lyricsosc.wxs -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
        -bindpath "app=$stage" -d Version=$version -o $msi
    if ($LASTEXITCODE) { throw "wix build failed" }
    # Guard: an MSI without the app inside is a few hundred KB; the real one is several MB
    if ((Get-Item $msi).Length -lt 2MB) { throw "MSI looks empty ($((Get-Item $msi).Length) bytes)" }
} else { Write-Warning "WiX not found: skipping .msi (dotnet tool install -g wix)" }
Remove-Item -Recurse -Force $stage
Write-Host "Done -> dist\" -ForegroundColor Green
