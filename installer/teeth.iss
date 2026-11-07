; Installer of Teeth, for Inno Setup 6.7 or later.
; Built by tools/build_installer.ps1, which passes the version and the executable:
;   ISCC /DAppVersion=1.0.1 /DExePath=C:\t\teeth\release\teeth.exe installer\teeth.iss
; It installs for the current user only, without administrator rights, and uninstalling
; leaves the user's samples, settings and unsaved takes where they are.

#ifndef AppVersion
  #error Falta /DAppVersion. Construye el instalador con tools/build_installer.ps1.
#endif
#ifndef ExePath
  #error Falta /DExePath. Construye el instalador con tools/build_installer.ps1.
#endif

[Setup]
; Fixed, so a new version replaces the installed one.
AppId={{6F1D3C52-8A47-4E0B-9C21-7B5E2A90D4F3}
AppName=Teeth
AppVersion={#AppVersion}
AppVerName=Teeth {#AppVersion}
DefaultDirName={autopf}\Teeth
DefaultGroupName=Teeth
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
; Setup and uninstall ask to close Teeth while it runs.
AppMutex=Local\Teeth-single-instance
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputBaseFilename=Teeth-Setup-{#AppVersion}
SetupIconFile=..\assets\icon\teeth.ico
UninstallDisplayIcon={app}\Teeth.exe
UninstallDisplayName=Teeth
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Teeth
VersionInfoDescription=Instalador de Teeth
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern dark
WizardSmallImageFile=art\wizard-small-58.png,art\wizard-small-77.png,art\wizard-small-97.png,art\wizard-small-116.png,art\wizard-small-124.png,art\wizard-small-143.png,art\wizard-small-159.png
WizardImageFile=art\wizard-202.png,art\wizard-269.png,art\wizard-336.png,art\wizard-403.png,art\wizard-430.png,art\wizard-498.png,art\wizard-534.png
WizardImageBackColor=$000000

[Languages]
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "Teeth.exe"; Flags: ignoreversion
; The Inter font's licence asks to travel with the font, which is inside the executable.
Source: "..\assets\fonts\OFL.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Teeth"; Filename: "{app}\Teeth.exe"
Name: "{autodesktop}\Teeth"; Filename: "{app}\Teeth.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Teeth.exe"; Description: "{cm:LaunchProgram,Teeth}"; Flags: nowait postinstall skipifsilent
