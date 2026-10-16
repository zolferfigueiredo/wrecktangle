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
; VersionInfoVersion takes numbers only, so a pre-release like 0.4.1-beta gives 0.4.1 there.
#if Pos("-", AppVersion) > 0
  #define NumericVersion Copy(AppVersion, 1, Pos("-", AppVersion) - 1)
#else
  #define NumericVersion AppVersion
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
; No AppMutex: [Code] closes a running copy itself instead of asking the user to.
CloseApplications=yes
RestartApplications=no
OutputBaseFilename=Wrecktangle-{#AppVersion}-{#Arch}-setup
OutputDir=..\dist
SetupIconFile=..\assets\wrecktangle.ico
UninstallDisplayIcon={app}\wrecktangle.exe
UninstallDisplayName=Wrecktangle
VersionInfoVersion={#NumericVersion}.0
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
; Only on a first install, so an update keeps what the user chose in Settings.
Name: "startup"; Description: "{cm:LaunchAtStartup}"; Flags: unchecked; Check: not IsUpgrade
; Windows lets only the user pin to the taskbar, so a desktop shortcut is what setup can offer.
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked; Check: not IsUpgrade

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "wrecktangle.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Wrecktangle"; Filename: "{app}\wrecktangle.exe"
Name: "{autodesktop}\Wrecktangle"; Filename: "{app}\wrecktangle.exe"; Tasks: desktopicon

[Registry]
; Same value name and quoted path as src/startup.rs writes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Wrecktangle"; ValueData: """{app}\wrecktangle.exe"""; Tasks: startup; Flags: uninsdeletevalue
; Covers a Run value the app itself created later from Settings, so uninstall always clears it.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Wrecktangle"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\wrecktangle.exe"; Description: "{cm:LaunchProgram,Wrecktangle}"; Flags: nowait postinstall skipifsilent
; A silent install is an in-app update: open the new version again, which then says it was updated.
Filename: "{app}\wrecktangle.exe"; Parameters: "--updated"; Flags: nowait; Check: RelaunchAfterSilentUpdate

[UninstallRun]
; The app moves the Run value above to a logon task named by task_name in src/startup.rs.
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /TN ""Wrecktangle ({username})"" /F"; Flags: runhidden; RunOnceId: "DeleteStartupTask"

[Code]
const
  WM_CLOSE = $0010;
  // The single-instance mutex in src/main.rs and the main window class in src/app.rs.
  AppMutexName = 'Local\Wrecktangle.SingleInstance';
  AppWindowClass = 'WrecktangleMainWindow';
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{5FC369EA-4EC3-4B23-9F2C-DAA462C9BD79}_is1';

var
  AppWasRunning: Boolean;

// Quits a running Wrecktangle the way its tray Quit does (WM_CLOSE to the main window),
// so nobody has to find it in the notification area first.
function CloseRunningApp(): Boolean;
var
  Wnd: HWND;
  Waited: Integer;
begin
  if not CheckForMutexes(AppMutexName) then
  begin
    Result := True;
    Exit;
  end;
  AppWasRunning := True;
  Wnd := FindWindowByClassName(AppWindowClass);
  if Wnd <> 0 then
    PostMessage(Wnd, WM_CLOSE, 0, 0);
  Waited := 0;
  while CheckForMutexes(AppMutexName) and (Waited < 5000) do
  begin
    Sleep(100);
    Waited := Waited + 100;
  end;
  Result := not CheckForMutexes(AppMutexName);
end;

function IsUpgrade(): Boolean;
begin
  Result := RegKeyExists(HKCU, UninstallKey);
end;

// Runs when installing starts, so cancelling the wizard leaves the app running.
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if CloseRunningApp() then
    Result := ''
  else
    Result := FmtMessage(SetupMessage(msgSetupAppRunningError), ['Wrecktangle']);
end;

function RelaunchAfterSilentUpdate(): Boolean;
begin
  Result := WizardSilent() and AppWasRunning;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    CloseRunningApp();
end;
