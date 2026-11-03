# Build release des-k as you, install it to Program Files, register the logon task that
# starts it as admin, and (re)start it. Run again from a normal terminal after every change.
# From a release zip (no Cargo.toml beside it) it installs the des-k.exe beside it instead.
# Admin so its keyboard hook still hears keys while an admin window has focus (spec 0008).
#
# The running copy is force-stopped ON PURPOSE: `des-k --quit` would show every hidden
# window, while a hard stop leaves state.txt behind so the new copy adopts the layout.
param(
    [switch]$Elevated, # internal: the admin half, started by this script
    [string]$User,     # internal: the account captured before elevating
    [string]$Log       # internal: where the admin half writes what it did
)
$ErrorActionPreference = 'Stop'
$dest = Join-Path $env:ProgramFiles 'des-k'
$exe = Join-Path $dest 'des-k.exe'
$fromSource = Test-Path (Join-Path $PSScriptRoot 'Cargo.toml')
$src = if ($fromSource) { Join-Path $PSScriptRoot 'target\release\des-k.exe' } else { Join-Path $PSScriptRoot 'des-k.exe' }

if ($Elevated) {
    function Say($msg) { Add-Content -LiteralPath $Log -Value $msg }
    try {
        # Hard kill before ending the task: ending it first might close des-k gently.
        # pdesk is des-k's old name: stop it the same way and remove its task and folder, so only
        # des-k starts at login. Its data folder moves to des-k's when des-k first starts.
        Get-Process des-k, pdesk -ErrorAction SilentlyContinue | Stop-Process -Force -PassThru | Wait-Process
        Stop-ScheduledTask -TaskName des-k -ErrorAction SilentlyContinue
        Unregister-ScheduledTask -TaskName pdesk -Confirm:$false -ErrorAction SilentlyContinue
        Remove-Item -Recurse (Join-Path $env:ProgramFiles 'pdesk') -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force $dest | Out-Null
        Copy-Item $src $exe -Force
        # A downloaded zip marks its files as from the internet; you chose to install this one.
        Unblock-File $exe

        $task = @{
            TaskName  = 'des-k'
            Action    = New-ScheduledTaskAction -Execute $exe
            Trigger   = New-ScheduledTaskTrigger -AtLogOn -User $User
            Principal = New-ScheduledTaskPrincipal -UserId $User -LogonType Interactive -RunLevel Highest
            # Task Scheduler's defaults stop it on battery, end it after 72 hours, and run it
            # below normal priority: all wrong for a tool that must answer keys at once.
            Settings  = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
                -ExecutionTimeLimit ([TimeSpan]::Zero) -Priority 4 -MultipleInstances IgnoreNew
        }
        Register-ScheduledTask @task -Force | Out-Null
        Start-ScheduledTask -TaskName des-k

        # The copy can lose a race with the old process (scope feature F): say so, loudly.
        foreach ($i in 1..20) {
            if (Get-Process des-k -ErrorAction SilentlyContinue) { Say "des-k started as admin from $exe"; exit 0 }
            Start-Sleep -Milliseconds 250
        }
        Say "ERROR: des-k is NOT running after install. Windows it hid stay hidden until it runs."
        exit 1
    } catch {
        Say "ERROR: $_"
        Say "des-k may not be running. Windows it hid stay hidden until it runs; run install.ps1 again."
        exit 1
    }
}

if ($fromSource) {
    if ([Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'run install.ps1 from a normal terminal: the build must not be owned by Administrators'
    }
    cargo build --release --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
    if ($LASTEXITCODE) { throw 'build failed' }
} elseif (-not (Test-Path $src)) {
    throw "no des-k.exe next to install.ps1 ($src): unzip the whole release first"
}

# Read now, never after elevating: an admin prompt answered by another account would change it.
$me = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$log = Join-Path $env:TEMP 'des-k-install.log'
Set-Content -LiteralPath $log -Value $null
$shell = (Get-Process -Id $PID).Path
$argv = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Elevated -User `"$me`" -Log `"$log`""
try {
    $p = Start-Process $shell -ArgumentList $argv -Verb RunAs -WindowStyle Hidden -Wait -PassThru
} catch {
    throw "admin prompt declined, nothing changed: $_"
}
Get-Content -LiteralPath $log | Write-Host
if ($p.ExitCode) { exit $p.ExitCode }

# Exactly one way starts des-k at login: the task. A leftover Run value or exe (both still named
# pdesk, from before the rename) could start a normal copy first.
Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name pdesk -ErrorAction SilentlyContinue
Remove-Item (Join-Path $env:LOCALAPPDATA 'pdesk\pdesk.exe'), (Join-Path $env:LOCALAPPDATA 'des-k\pdesk.exe') -ErrorAction SilentlyContinue
Write-Host "des-k installed to $exe, starts as admin at login. Stop it with: & '$exe' --quit"
