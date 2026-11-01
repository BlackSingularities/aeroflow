; Instalator AeroFlow (Inno Setup 6). Budowany automatycznie w GitHub Actions:
;   ISCC.exe /DAppVersion=1.2.3 installer\aeroflow.iss
; Instalacja do Program Files z uprawnieniami administratora (UAC). Aktualizacja z programu
; uruchamia ten sam instalator po cichu – trafia do folderu poprzedniej instalacji.

#define AppName "AeroFlow"
#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif
#define AppExe "aeroflow.exe"
#define AppUrl "https://redzikowski.com/portfolio/aeroflow"

[Setup]
AppId={{8C3A6F1E-5B7D-4E2A-9F41-AE20F10A7A11}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Adam Rędzikowski
AppPublisherURL={#AppUrl}
AppSupportURL=https://redzikowski.com
AppUpdatesURL={#AppUrl}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
OutputDir=..\dist
OutputBaseFilename=AeroFlow-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
UsePreviousAppDir=yes
SetupLogging=yes
CloseApplications=force
RestartApplications=no
ChangesAssociations=yes
LicenseFile=..\LICENSE
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} — tunel aerodynamiczny 2D

[Languages]
Name: "polish"; MessagesFile: "compiler:Languages\Polish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
polish.AssocTask=Otwieraj sceny .aeroflow w programie AeroFlow
polish.AssocGroup=Skojarzenia plików:
polish.SceneType=Scena AeroFlow
english.AssocTask=Open .aeroflow scenes with AeroFlow
english.AssocGroup=File associations:
english.SceneType=AeroFlow scene

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "assoc"; Description: "{cm:AssocTask}"; GroupDescription: "{cm:AssocGroup}"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.aeroflow"; ValueType: string; ValueName: ""; ValueData: "AeroFlow.Scene"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\AeroFlow.Scene"; ValueType: string; ValueName: ""; ValueData: "{cm:SceneType}"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\AeroFlow.Scene\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\AeroFlow.Scene\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: assoc

[Run]
; instalacja ręczna: pole „Uruchom AeroFlow” na ostatniej stronie
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent runasoriginaluser
; aktualizacja z programu (tryb cichy z /RELAUNCH=1): ponowne uruchomienie jako zwykły użytkownik
Filename: "{app}\{#AppExe}"; Flags: nowait runasoriginaluser; Check: ShouldRelaunch

[Code]
const
  OldKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{8C3A6F1E-5B7D-4E2A-9F41-AE20F10A7A11}_is1';

function ShouldRelaunch: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:RELAUNCH|0}') = '1');
end;

// Wersje do 1.1.2 instalowały się domyślnie dla bieżącego użytkownika (wpis w HKCU) – usuwamy
// taką kopię, żeby po przejściu do Program Files nie zostały dwa programy.
procedure RemoveOldPerUserCopy();
var
  Uninst: String;
  Code: Integer;
begin
  if RegQueryStringValue(HKCU, OldKey, 'UninstallString', Uninst) then
  begin
    Exec(RemoveQuotes(Uninst), '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART', '', SW_HIDE, ewWaitUntilTerminated, Code);
    RegDeleteKeyIncludingSubkeys(HKCU, OldKey);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    RemoveOldPerUserCopy();
end;
