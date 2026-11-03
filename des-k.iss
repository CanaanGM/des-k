; The double-click installer and its uninstaller, built with Inno Setup from a release build:
;   cargo build --release
;   iscc /DAppVersion=0.2.0 des-k.iss        -> target\setup\des-k-v0.2.0-setup.exe
; It does what install.ps1's admin half does (spec 0008): hard stop des-k, copy it to Program
; Files, register the logon task that starts it as admin, start it, and say so if it is not running.
#ifndef AppVersion
  #error Pass the version: iscc /DAppVersion=x.y.z des-k.iss
#endif

[Setup]
AppId={{246A26B4-9BC0-4230-95AC-DE1E0852CB1A}
AppName=des-k
AppVersion={#AppVersion}
AppPublisherURL=https://canaangm.github.io/des-k/
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
; The README, AGENTS.md and the task all expect C:\Program Files\des-k.
DefaultDirName={autopf}\des-k
; Same AppId as when it was named pdesk, so Setup would reuse C:\Program Files\pdesk otherwise.
UsePreviousAppDir=no
DisableDirPage=yes
DisableProgramGroupPage=yes
; Restart Manager would close des-k gently, which shows every hidden window and forgets the
; layout. PrepareToInstall hard stops it instead.
CloseApplications=no
; HKCU and {localappdata} below are the installing account's on purpose: InitializeSetup
; refuses to go on unless that is the account that started Setup.
UsedUserAreasWarning=no
UninstallDisplayIcon={app}\des-k.exe
OutputDir=target\setup
OutputBaseFilename=des-k-v{#AppVersion}-setup
WizardStyle=modern

[Files]
Source: "target\release\des-k.exe"; DestDir: "{app}"; Flags: ignoreversion

; Exactly one way starts des-k at login: the task. A leftover Run value or old exe (named pdesk,
; from before the rename) could start a normal copy first and win the single instance check.
[InstallDelete]
Type: files; Name: "{localappdata}\pdesk\pdesk.exe"
Type: files; Name: "{localappdata}\des-k\pdesk.exe"
Type: filesandordirs; Name: "{autopf}\pdesk"

; Starts des-k again after a Quit or a crash, through its task so it runs as admin
; (des-k --start); a plain des-k.exe here would start a normal copy. Same entry as install.ps1's.
[Icons]
Name: "{userprograms}\des-k"; Filename: "{app}\des-k.exe"; Parameters: "--start"; Comment: "Start des-k as admin, through its logon task"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "pdesk"; Flags: deletevalue

[Code]
var
  User: String; // the account the task runs as, as whoami prints it (DOMAIN\name)

function Run(const Exe, Params: String): Integer;
begin
  if not Exec(Exe, Params, '', SW_HIDE, ewWaitUntilTerminated, Result) then
    Result := -1;
end;

function Running: Boolean;
begin
  Result := Run(ExpandConstant('{cmd}'),
    '/c tasklist /fi "imagename eq des-k.exe" /nh | findstr /i /b des-k.exe >nul') = 0;
end;

// Polls for up to Tenths/10 seconds until des-k is running (Want) or gone (not Want).
function WaitFor(const Want: Boolean; const Tenths: Integer): Boolean;
var
  I: Integer;
begin
  for I := 1 to Tenths do begin
    if Running = Want then begin
      Result := True;
      exit;
    end;
    Sleep(100);
  end;
  Result := Running = Want;
end;

function InitializeSetup: Boolean;
var
  Code: Integer;
  Output: TExecOutput;
begin
  Result := False;
  if not ExecAndCaptureOutput('whoami.exe', '', '', SW_HIDE, ewWaitUntilTerminated, Code, Output)
    or (Code <> 0) or (GetArrayLength(Output.StdOut) = 0) then begin
    SuppressibleMsgBox('Cannot tell which account is installing des-k. Nothing changed.', mbError, MB_OK, IDOK);
    exit;
  end;
  User := Trim(Output.StdOut[0]);
  // Spec 0008: the task belongs to the account that started Setup, never one typed into an
  // over the shoulder admin prompt. That side checks with its own token, so no file is trusted.
  if not ExecAsOriginalUser(ExpandConstant('{cmd}'), '/c whoami | findstr /i /x /c:"' + User + '" >nul',
    '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then begin
    SuppressibleMsgBox('The admin prompt was answered by a different account (' + User + '). '
      + 'Install des-k while signed in to the account that will use it, and that account must be an admin. Nothing changed.',
      mbError, MB_OK, IDOK);
    exit;
  end;
  Result := True;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  // Hard kill ON PURPOSE: Quit would show every hidden window, while a kill leaves state.txt
  // behind so the new copy adopts the layout. Kill before ending the task, which might close it gently.
  Run('taskkill.exe', '/f /im des-k.exe');
  Run('schtasks.exe', '/end /tn des-k');
  // pdesk is des-k's old name: same hard stop, and its task goes, so only des-k starts at login.
  // Its data folder moves to des-k's when des-k first starts.
  Run('taskkill.exe', '/f /im pdesk.exe');
  Run('schtasks.exe', '/delete /tn pdesk /f');
  if not WaitFor(False, 50) then
    Result := 'des-k is still running and could not be stopped. Nothing changed.';
end;

// Same task install.ps1 registers. Plain ASCII with no <?xml?> line: schtasks rejects UTF-8.
function TaskXml: String;
begin
  Result :=
    '<Task version="1.3" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">'
    + '<Principals><Principal id="Author"><UserId>' + User + '</UserId>'
    + '<LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>'
    // Task Scheduler's defaults stop it on battery, end it after 72 hours, and run it below
    // normal priority: all wrong for a tool that must answer keys at once.
    + '<Settings><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>'
    + '<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit>'
    + '<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><Priority>4</Priority>'
    + '<UseUnifiedSchedulingEngine>true</UseUnifiedSchedulingEngine></Settings>'
    + '<Triggers><LogonTrigger><UserId>' + User + '</UserId></LogonTrigger></Triggers>'
    + '<Actions Context="Author"><Exec><Command>' + ExpandConstant('{app}\des-k.exe') + '</Command></Exec></Actions>'
    + '</Task>';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Xml: String;
begin
  if CurStep <> ssPostInstall then exit;
  Xml := ExpandConstant('{tmp}\des-k-task.xml');
  if not SaveStringToFile(Xml, TaskXml, False)
    or (Run('schtasks.exe', '/create /tn des-k /xml "' + Xml + '" /f') <> 0) then begin
    SuppressibleMsgBox('ERROR: could not register the des-k logon task, so des-k will not start at login. '
      + 'Windows it hid stay hidden until it runs. Run this setup again.', mbError, MB_OK, IDOK);
    exit;
  end;
  Run('schtasks.exe', '/run /tn des-k');
  // The copy can lose a race with the old process (scope feature F): say so, loudly.
  if not WaitFor(True, 50) then
    SuppressibleMsgBox('ERROR: des-k is NOT running after install. Windows it hid stay hidden until it runs. '
      + 'Run this setup again.', mbError, MB_OK, IDOK);
end;

function InitializeUninstall: Boolean;
begin
  Result := True;
  if not Running then begin
    // Stopped by a crash or a kill: --quit still shows what it hid and gives Windows its
    // touchpad gestures back.
    Run(ExpandConstant('{app}\des-k.exe'), '--quit');
    exit;
  end;
  // Quit, not a kill: it shows every hidden window and hands Windows its touchpad gestures back.
  Run(ExpandConstant('{app}\des-k.exe'), '--quit');
  if not WaitFor(False, 100) then begin
    SuppressibleMsgBox('des-k did not quit, so nothing was removed. Quit it from the tray, then uninstall again.',
      mbError, MB_OK, IDOK);
    Result := False;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    Run('schtasks.exe', '/delete /tn des-k /f');
end;
