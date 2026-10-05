; Installer for Wrecktangle. installer.bat and .github/workflows/release.yml compile this with /DAppVersion,
; and the workflow with /DArch=x64 or /DArch=x86 and /DSourceExe pointing at that architecture's build.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\wrecktangle.exe"
#endif

[Setup]
AppId={{5FC369EA-4EC3-4B23-9F2C-DAA462C9BD79}
AppName=Wrecktangle
AppVersion={#AppVersion}
AppVerName=Wrecktangle {#AppVersion}
AppPublisher=Zolfer Figueiredo
AppPublisherURL=https://zolfer.com
AppSupportURL=https://github.com/zolferfigueiredo/wrecktangle/issues
AppUpdatesURL=https://github.com/zolferfigueiredo/wrecktangle/releases
DefaultDirName={autopf}\Wrecktangle
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
#if Arch == "x86"
; The 32-bit build runs on any Windows, 64-bit included.
ArchitecturesAllowed=x86compatible
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
AppMutex=Local\Wrecktangle.SingleInstance
CloseApplications=yes
RestartApplications=no
OutputBaseFilename=Wrecktangle-{#AppVersion}-{#Arch}-setup
OutputDir=..\dist
SetupIconFile=..\assets\wrecktangle.ico
UninstallDisplayIcon={app}\wrecktangle.exe
UninstallDisplayName=Wrecktangle
VersionInfoVersion={#AppVersion}.0
WizardStyle=modern
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "italian"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "polish"; MessagesFile: "compiler:Languages\Polish.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "ukrainian"; MessagesFile: "compiler:Languages\Ukrainian.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"

; The same wording as "general.startup" in lang/*.json.
[CustomMessages]
english.LaunchAtStartup=Launch at startup
german.LaunchAtStartup=Beim Start ausführen
spanish.LaunchAtStartup=Abrir al iniciar
french.LaunchAtStartup=Ouvrir au démarrage
italian.LaunchAtStartup=Apri all’avvio
polish.LaunchAtStartup=Uruchamiaj przy starcie
brazilianportuguese.LaunchAtStartup=Abrir ao iniciar
russian.LaunchAtStartup=Запускать при старте
ukrainian.LaunchAtStartup=Запускати під час старту
japanese.LaunchAtStartup=起動時に開く
korean.LaunchAtStartup=시작할 때 실행

[Tasks]
Name: "startup"; Description: "{cm:LaunchAtStartup}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "wrecktangle.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Wrecktangle"; Filename: "{app}\wrecktangle.exe"

[Registry]
; Same value name and quoted path as src/startup.rs writes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Wrecktangle"; ValueData: """{app}\wrecktangle.exe"""; Tasks: startup; Flags: uninsdeletevalue
; Covers a Run value the app itself created later from Settings, so uninstall always clears it.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Wrecktangle"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\wrecktangle.exe"; Description: "{cm:LaunchProgram,Wrecktangle}"; Flags: nowait postinstall skipifsilent
