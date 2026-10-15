; Inno Setup script for logibat-tray.
; Build: iscc /DAppVersion=0.1.0 installer\logibat-tray.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "LogiBat Tray"
#define AppExe "logibat-tray.exe"

[Setup]
AppId={{6E0B9C1A-6F0E-4C3B-9D5E-2B7C1C4A8F21}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=allgoewer
AppPublisherURL=https://github.com/allgoewer/logibat-tray
DefaultDirName={localappdata}\Programs\LogiBat Tray
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=Output
OutputBaseFilename=LogiBatTray-Setup-{#AppVersion}
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Must match MUTEX_NAME in src/app.rs.
AppMutex=LogiBatTrayMutex
CloseApplications=yes

[Tasks]
Name: "autostart"; Description: "Start {#AppName} with Windows"; GroupDescription: "Additional options:"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"

[Registry]
; Same key and value name as src/autostart.rs, so the tray menu toggle and the installer stay in sync.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "LogiBatTray"; ValueData: """{app}\{#AppExe}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName} now"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\logibat-tray"
