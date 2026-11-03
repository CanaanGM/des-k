; The double-click installer and its uninstaller, built with Inno Setup from a release build:
;   cargo build --release
;   iscc /DAppVersion=0.2.0 pdesk.iss        -> target\setup\pdesk-v0.2.0-setup.exe
; It does what install.ps1's admin half does (spec 0008): hard stop pdesk, copy it to Program
; Files, register the logon task that starts it as admin, start it, and say so if it is not running.
#ifndef AppVersion
  #error Pass the version: iscc /DAppVersion=x.y.z pdesk.iss
#endif

[Setup]
AppId={{246A26B4-9BC0-4230-95AC-DE1E0852CB1A}
AppName=pdesk
AppVersion={#AppVersion}
AppPublisherURL=https://canaangm.github.io/pdesk/
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
; The README, AGENTS.md and the task all expect C:\Program Files\pdesk.
DefaultDirName={autopf}\pdesk
DisableDirPage=yes
DisableProgramGroupPage=yes
; Restart Manager would close pdesk gently, which shows every hidden window and forgets the
; layout. PrepareToInstall hard stops it instead.
CloseApplications=no
; HKCU and {localappdata} below are the installing account's on purpose: InitializeSetup
; refuses to go on unless that is the account that started Setup.
UsedUserAreasWarning=no
UninstallDisplayIcon={app}\pdesk.exe
OutputDir=target\setup
OutputBaseFilename=pdesk-v{#AppVersion}-setup
WizardStyle=modern

[Files]
Source: "target\release\pdesk.exe"; DestDir: "{app}"; Flags: ignoreversion

; Exactly one way starts pdesk at login: the task. A leftover Run value or old exe could start
; a normal copy first and win the single instance check.
[InstallDelete]
Type: files; Name: "{localappdata}\pdesk\pdesk.exe"

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
    '/c tasklist /fi "imagename eq pdesk.exe" /nh | findstr /i /b pdesk.exe >nul') = 0;
end;

// Polls for up to Tenths/10 seconds until pdesk is running (Want) or gone (not Want).
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
    SuppressibleMsgBox('Cannot tell which account is installing pdesk. Nothing changed.', mbError, MB_OK, IDOK);
    exit;
  end;
  User := Trim(Output.StdOut[0]);
  // Spec 0008: the task belongs to the account that started Setup, never one typed into an
  // over the shoulder admin prompt. That side checks with its own token, so no file is trusted.
  if not ExecAsOriginalUser(ExpandConstant('{cmd}'), '/c whoami | findstr /i /x /c:"' + User + '" >nul',
    '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then begin
    SuppressibleMsgBox('The admin prompt was answered by a different account (' + User + '). '
      + 'Install pdesk while signed in to the account that will use it, and that account must be an admin. Nothing changed.',
      mbError, MB_OK, IDOK);
    exit;
  end;
  Result := True;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  // Hard kill ON PURPOSE: Quit would show every hidden window, while a kill leaves state.txt
  // behind so the new copy adopts the layout. Kill before ending the task, which might close it gently.
  Run('taskkill.exe', '/f /im pdesk.exe');
  Run('schtasks.exe', '/end /tn pdesk');
  if not WaitFor(False, 50) then
    Result := 'pdesk is still running and could not be stopped. Nothing changed.';
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
    + '<Actions Context="Author"><Exec><Command>' + ExpandConstant('{app}\pdesk.exe') + '</Command></Exec></Actions>'
    + '</Task>';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Xml: String;
begin
  if CurStep <> ssPostInstall then exit;
  Xml := ExpandConstant('{tmp}\pdesk-task.xml');
  if not SaveStringToFile(Xml, TaskXml, False)
    or (Run('schtasks.exe', '/create /tn pdesk /xml "' + Xml + '" /f') <> 0) then begin
    SuppressibleMsgBox('ERROR: could not register the pdesk logon task, so pdesk will not start at login. '
      + 'Windows it hid stay hidden until it runs. Run this setup again.', mbError, MB_OK, IDOK);
    exit;
  end;
  Run('schtasks.exe', '/run /tn pdesk');
  // The copy can lose a race with the old process (scope feature F): say so, loudly.
  if not WaitFor(True, 50) then
    SuppressibleMsgBox('ERROR: pdesk is NOT running after install. Windows it hid stay hidden until it runs. '
      + 'Run this setup again.', mbError, MB_OK, IDOK);
end;

function InitializeUninstall: Boolean;
begin
  Result := True;
  if not Running then exit;
  // Quit, not a kill: it shows every hidden window and hands Windows its touchpad gestures back.
  Run(ExpandConstant('{app}\pdesk.exe'), '--quit');
  if not WaitFor(False, 100) then begin
    SuppressibleMsgBox('pdesk did not quit, so nothing was removed. Quit it from the tray, then uninstall again.',
      mbError, MB_OK, IDOK);
    Result := False;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    Run('schtasks.exe', '/delete /tn pdesk /f');
end;
