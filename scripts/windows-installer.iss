#if Ver < EncodeVer(6, 5, 0)
  #error Inno Setup 6.5 or newer is required for authenticated component downloads.
#endif
[Setup]
AppId={{8A65B5A7-79A4-4EBF-A89E-9B8F745FA96F}
AppName=Stashi Wallet
AppVersion={#AppVersion}
AppPublisher=Pirate Chain
AppPublisherURL=https://piratechain.com
DefaultDirName={localappdata}\StashiWallet
DefaultGroupName=Stashi Wallet
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\Stashi Wallet.exe

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "full"; Description: "Wallet with optional privacy tools"
Name: "compact"; Description: "Wallet with built-in Tor"
Name: "custom"; Description: "Choose privacy tools"; Flags: iscustom

[Components]
Name: "wallet"; Description: "Stashi Wallet (includes Tor)"; Types: full compact custom; Flags: fixed
Name: "i2p"; Description: "I2P router (optional download)"; Types: full
Name: "bridges"; Description: "Tor bridge helpers for restricted networks (optional download)"; Types: full

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Excludes: "\i2p\*,\tor-pt\*,*.lib,*.exp,*.pdb"; Flags: recursesubdirs createallsubdirs ignoreversion; Components: wallet
#include ComponentsFile

[InstallDelete]
Type: files; Name: "{app}\app.exe"

[Icons]
Name: "{autoprograms}\Stashi Wallet"; Filename: "{app}\Stashi Wallet.exe"
Name: "{autodesktop}\Stashi Wallet"; Filename: "{app}\Stashi Wallet.exe"

[Run]
Filename: "{app}\Stashi Wallet.exe"; Description: "Launch Stashi Wallet"; Flags: nowait postinstall skipifsilent
