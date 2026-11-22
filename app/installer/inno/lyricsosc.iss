; LyricsOSC — Windows installer (Inno Setup 6)
; Build:  iscc installer\inno\lyricsosc.iss   (after scripts\build_release.ps1 created dist\)
; The user picks the install folder; the voice engine (Python + Whisper + CUDA if NVIDIA)
; is installed automatically at the end, inside that folder.

#define AppName "LyricsOSC"
#ifndef AppVersion
  #define AppVersion "1.1.0"
#endif
#define Dist "..\..\dist\windows"

[Setup]
AppId={{6C2E4C8B-5A3F-4C61-9E0B-4C5D9A1F7E21}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Happyuky7
AppPublisherURL=https://lyricsosc.happyuky7.com
AppSupportURL=https://github.com/Happyuky7/LyricsOSC/issues
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
; Let the user choose any folder (also without admin rights)
DisableDirPage=no
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
LicenseFile=..\..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=LyricsOSC-{#AppVersion}-setup
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\LyricsOSC.exe
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "pt"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "ko"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "pl"; MessagesFile: "compiler:Languages\Polish.isl"
Name: "tr"; MessagesFile: "compiler:Languages\Turkish.isl"
Name: "uk"; MessagesFile: "compiler:Languages\Ukrainian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "voice"; Description: "Voice engine (Whisper) — Python + AI model, ~0.5 GB (+1 GB with NVIDIA GPU)"; GroupDescription: "Components:"
Name: "voice\nogpu"; Description: "Force CPU (do not install CUDA even if an NVIDIA GPU is found)"; Flags: unchecked

[Files]
; App files only. The voice engine (python\.venv, models...) is downloaded on the user's PC by
; setup_voice.ps1, so anything a local test install left in dist\windows is excluded here.
Source: "{#Dist}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; \
  Excludes: "\python\.venv\*,\python\runtime\*,\python\uv-cache\*,\models\*,\tools\*,\cache\*,\config.json,\lyricsosc.log,*.pyc,__pycache__"

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\LyricsOSC.exe"
Name: "{group}\Repair voice engine"; Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\setup_voice.ps1"" -InstallDir ""{app}"""
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\LyricsOSC.exe"; Tasks: desktopicon

[Run]
; Automatic voice-engine setup (visible console so the user sees progress).
; Skipped when the app updates itself (/UPDATE): the voice engine is already installed.
Filename: "powershell.exe"; \
  Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\setup_voice.ps1"" -InstallDir ""{app}"""; \
  StatusMsg: "Installing the voice engine (Python + Whisper)…"; Tasks: voice and not voice\nogpu; Flags: waituntilterminated; Check: NeedsVoiceSetup
Filename: "powershell.exe"; \
  Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\setup_voice.ps1"" -InstallDir ""{app}"" -NoGpu"; \
  StatusMsg: "Installing the voice engine (Python + Whisper)…"; Tasks: voice\nogpu; Flags: waituntilterminated; Check: NeedsVoiceSetup
Filename: "{app}\LyricsOSC.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
; After a self-update (/UPDATE, run silently by the app) open LyricsOSC again
Filename: "{app}\LyricsOSC.exe"; WorkingDir: "{app}"; Flags: nowait; Check: IsUpdate

[UninstallDelete]
; Everything the setup script downloaded into the install folder
Type: filesandordirs; Name: "{app}\python\.venv"
Type: filesandordirs; Name: "{app}\python\runtime"
Type: filesandordirs; Name: "{app}\python\uv-cache"
Type: filesandordirs; Name: "{app}\models"
Type: filesandordirs; Name: "{app}\tools"
Type: filesandordirs; Name: "{app}\cache"
Type: files; Name: "{app}\config.json"
Type: files; Name: "{app}\lyricsosc.log"

[Code]
// /UPDATE is passed by LyricsOSC when it installs a downloaded update of itself.
function IsUpdate(): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/UPDATE') = 0 then
      Result := True;
end;

// Voice engine: set it up on a normal install, or on an update if it's missing.
function NeedsVoiceSetup(): Boolean;
begin
  Result := (not IsUpdate()) or (not FileExists(ExpandConstant('{app}\python\.venv\Scripts\python.exe')));
end;
