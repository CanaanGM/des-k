# The recovery drill's outside check (spec 0034, RELEASING.md). It holds no des-k code: it
# judges des-k the way you would, by whether a window you had open is still hidden.
#   powershell -File drill.ps1 snap                 before a fault: the windows you can see now
#   powershell -File drill.ps1 check                after recovery: PASS, or FAIL naming each still hidden
#   powershell -File drill.ps1 fail-saves           every state.txt save fails, as on a full disk
#   powershell -File drill.ps1 unfail-saves         saves work again
#   powershell -File drill.ps1 kill-setup <setup>   (admin) a setup killed mid update
# Windows are named by their exe file name and handle, never by their title.
param(
    [Parameter(Mandatory, Position = 0)][ValidateSet('snap', 'check', 'fail-saves', 'unfail-saves', 'kill-setup')][string]$Command,
    [Parameter(Position = 1)][string]$Setup
)
$ErrorActionPreference = 'Stop'

$snapshot = Join-Path $env:TEMP 'des-k-drill.json'
$blocker = Join-Path $env:LOCALAPPDATA 'des-k\state.tmp'
# Fewer still open than this after a fault, and a PASS would prove nothing.
$least = 3

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class DeskDrill {
    [ComImport, Guid("a5cd92ff-29be-454c-8d04-d82879fb3f1b"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IVirtualDesktopManager {
        [PreserveSig] int IsWindowOnCurrentVirtualDesktop(IntPtr hwnd, out int onCurrent);
        [PreserveSig] int GetWindowDesktopId(IntPtr hwnd, out Guid id);
        [PreserveSig] int MoveWindowToDesktop(IntPtr hwnd, ref Guid id);
    }
    [ComImport, Guid("aa509086-5ca9-4c25-8f95-589d3c07b48a")]
    class VirtualDesktopManager {}

    delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr hwnd, uint cmd);
    [DllImport("user32.dll")] static extern IntPtr GetWindowLongPtrW(IntPtr hwnd, int index);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out int value, int size);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool QueryFullProcessImageNameW(IntPtr h, uint flags, StringBuilder name, ref uint size);

    const uint GW_OWNER = 4;
    const int GWL_EXSTYLE = -20;
    const long WS_EX_TOOLWINDOW = 0x80;
    const int DWMWA_CLOAKED = 14;
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;

    static IVirtualDesktopManager vdm = (IVirtualDesktopManager)new VirtualDesktopManager();

    public static uint Pid(IntPtr hwnd) { uint pid; GetWindowThreadProcessId(hwnd, out pid); return pid; }

    public static int Cloaked(IntPtr hwnd) { int v; return DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, out v, 4) == 0 ? v : 0; }

    // A window on another Windows desktop is cloaked by Windows, not by des-k: never judged.
    public static bool OnCurrentDesktop(IntPtr hwnd) { int on; return vdm.IsWindowOnCurrentVirtualDesktop(hwnd, out on) == 0 && on != 0; }

    public static string Exe(uint pid) {
        IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (h == IntPtr.Zero) return "?";
        try {
            var sb = new StringBuilder(1024);
            uint size = (uint)sb.Capacity;
            if (!QueryFullProcessImageNameW(h, 0, sb, ref size)) return "?";
            return System.IO.Path.GetFileName(sb.ToString()).ToLowerInvariant();
        } finally { CloseHandle(h); }
    }

    // What you can see now: top level app windows, in view, on this Windows desktop.
    public static List<IntPtr> Seen() {
        var list = new List<IntPtr>();
        EnumWindows((h, _) => {
            if (IsWindowVisible(h) && !IsIconic(h) && GetWindow(h, GW_OWNER) == IntPtr.Zero
                && ((long)GetWindowLongPtrW(h, GWL_EXSTYLE) & WS_EX_TOOLWINDOW) == 0
                && Cloaked(h) == 0 && OnCurrentDesktop(h)) list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
'@

function Test-Admin {
    ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

switch ($Command) {
    'snap' {
        $seen = foreach ($h in [DeskDrill]::Seen()) {
            $pid_ = [DeskDrill]::Pid($h)
            [pscustomobject]@{ hwnd = [int64]$h; pid = [int64]$pid_; exe = [DeskDrill]::Exe($pid_) }
        }
        $seen = @($seen)
        ConvertTo-Json -InputObject $seen -Depth 2 | Set-Content -Path $snapshot -Encoding UTF8
        "snap: $($seen.Count) window(s) in view, saved to $snapshot"
        if ($seen.Count -lt $least) { "snap: open at least $least windows before a fault, or check cannot judge it" }
    }
    'check' {
        if (-not (Test-Path $snapshot)) {
            'FAIL: no snapshot, run snap first'
            exit 1
        }
        # Windows PowerShell hands the whole array on as one item: ForEach-Object opens it.
        $snap = @(Get-Content -Raw -Path $snapshot | ConvertFrom-Json | ForEach-Object { $_ })
        $open = @($snap | Where-Object {
                $h = [IntPtr][int64]$_.hwnd
                [DeskDrill]::IsWindow($h) -and [DeskDrill]::Pid($h) -eq [uint32]$_.pid -and [DeskDrill]::OnCurrentDesktop($h)
            })
        $closed = $snap.Count - $open.Count
        $hidden = @($open | Where-Object { [DeskDrill]::Cloaked([IntPtr][int64]$_.hwnd) -ne 0 })
        foreach ($w in $hidden) { "still hidden: $($w.exe) 0x{0:x}" -f [int64]$w.hwnd }
        if ($open.Count -lt $least) {
            "FAIL: too few windows to judge ($($open.Count) of $($snap.Count) still open, $closed closed since snap)"
            exit 1
        }
        if ($hidden.Count -gt 0) {
            "FAIL: $($hidden.Count) of $($open.Count) still hidden, $closed closed since snap"
            exit 1
        }
        "PASS: all $($open.Count) in view, $closed closed since snap"
        exit 0
    }
    'fail-saves' {
        # A folder where des-k writes state.tmp: every save fails through the same error path as a
        # full disk, and the old state.txt stays whole.
        if (Test-Path $blocker -PathType Leaf) { Remove-Item -LiteralPath $blocker }
        New-Item -ItemType Directory -Path $blocker -Force | Out-Null
        "fail-saves: every state.txt save fails until unfail-saves"
    }
    'unfail-saves' {
        if (Test-Path $blocker -PathType Container) { Remove-Item -LiteralPath $blocker -Recurse }
        "unfail-saves: saves work again"
    }
    'kill-setup' {
        if (-not (Test-Admin)) { throw 'kill-setup needs an admin terminal: the setup asks for admin' }
        if (-not $Setup -or -not (Test-Path $Setup -PathType Leaf)) { throw 'kill-setup needs the path of a des-k setup exe' }
        $p = Start-Process -FilePath $Setup -ArgumentList '/VERYSILENT' -PassThru
        "kill-setup: setup started (pid $($p.Id)), waiting for it to stop des-k"
        $until = (Get-Date).AddSeconds(60)
        while ((Get-Process -Name 'des-k' -ErrorAction SilentlyContinue) -and (Get-Date) -lt $until) { Start-Sleep -Milliseconds 100 }
        if (Get-Process -Name 'des-k' -ErrorAction SilentlyContinue) { throw 'kill-setup: des-k still runs after 60 s, nothing killed' }
        # Inno Setup's exe runs its real work as a child (setup.tmp): the whole tree goes, as a
        # crash or a power cut would end it.
        $kids = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($p.Id)" | ForEach-Object { $_.ProcessId })
        foreach ($id in @($p.Id) + $kids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
        "kill-setup: des-k stopped, setup killed ($(1 + $kids.Count) process(es)). des-k.exe in Program Files: $(if (Test-Path "$env:ProgramFiles\des-k\des-k.exe") { 'present' } else { 'missing' })"
    }
}
