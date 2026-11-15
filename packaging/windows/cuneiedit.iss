; Inno Setup script for the CuneiEdit installer (Inno Setup 6):
;   ISCC.exe /DAppVersion=<version> /DSourceExe=<path to cuneiedit.exe> packaging\windows\cuneiedit.iss
; Writes target\installer\CuneiEdit-<version>-windows-x64-setup.exe. The icon is expected at
; target\installer\CuneiEdit.ico (`cargo run -p cuneiedit-app --example app_icon -- <file>.ico`).
;
; Installs for the current user without administrator rights (or for all users, if chosen),
; registers the chosen file types through CuneiEdit itself and removes them when uninstalled.

#ifndef AppVersion
  #error Define AppVersion, e.g. /DAppVersion=0.3.2
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\cuneiedit.exe"
#endif

[Setup]
AppId={{65A0AD18-D3CA-40F3-8284-70B8ED7A2591}
AppName=CuneiEdit
AppVersion={#AppVersion}
AppVerName=CuneiEdit {#AppVersion}
AppPublisher=CuneiEdit
AppPublisherURL=https://github.com/MrMarkus42/cuneiedit
AppSupportURL=https://github.com/MrMarkus42/cuneiedit/issues
AppUpdatesURL=https://github.com/MrMarkus42/cuneiedit/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\CuneiEdit
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\..\target\installer
OutputBaseFilename=CuneiEdit-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\target\installer\CuneiEdit.ico
UninstallDisplayIcon={app}\CuneiEdit.exe
UninstallDisplayName=CuneiEdit
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "CuneiEdit.exe"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\CuneiEdit"; Filename: "{app}\CuneiEdit.exe"
Name: "{autodesktop}\CuneiEdit"; Filename: "{app}\CuneiEdit.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

#include "file-types.iss"

[Run]
; Registers exactly the chosen types (and removes the others) for the user who started setup.
Filename: "{app}\CuneiEdit.exe"; Parameters: "--register-file-types{code:ChosenFileTypes}"; Flags: runhidden waituntilterminated runasoriginaluser
Filename: "{app}\CuneiEdit.exe"; Description: "{cm:LaunchProgram,CuneiEdit}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{app}\CuneiEdit.exe"; Parameters: "--unregister-file-types"; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterFileTypes"

[UninstallDelete]
; Left behind by an update that replaced the running executable.
Type: files; Name: "{app}\CuneiEdit.exe.old"
