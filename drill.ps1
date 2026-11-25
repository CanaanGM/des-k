# The recovery drill's outside check (spec 0034, RELEASING.md). It holds no des-k code: it
# judges des-k the way you would, by whether a window you had open is still hidden.
#   powershell -File drill.ps1 snap                 before a fault: the windows you can see now
#   powershell -File drill.ps1 check                after recovery: PASS, or FAIL naming each still hidden
#   powershell -File drill.ps1 fail-saves           every state.txt save fails, as on a full disk
#   powershell -File drill.ps1 unfail-saves         saves work again
#   powershell -File drill.ps1 kill-setup <setup>   (admin) a setup killed mid update
#   powershell -File drill.ps1 stress [-Steps n]    (admin) the stress run (spec 0037): PASS, FAIL or INVALID
# Windows are named by their exe file name and handle, never by their title.
param(
    [Parameter(Mandatory, Position = 0)][ValidateSet('snap', 'check', 'fail-saves', 'unfail-saves', 'kill-setup', 'stress')][string]$Command,
    [Parameter(Position = 1)][string]$Setup,
    [ValidateRange(2, 1000000)][int]$Steps = 10000
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

    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x; public int y; }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort vk; public ushort scan; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint data; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }

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
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint flags);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromPoint(POINT pt, uint flags);
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint count, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] static extern uint GetGuiResources(IntPtr process, uint flags);
    [DllImport("kernel32.dll")] static extern uint SetThreadExecutionState(uint flags);

    const uint GW_OWNER = 4;
    const int GWL_EXSTYLE = -20;
    const long WS_EX_TOOLWINDOW = 0x80;
    const int DWMWA_CLOAKED = 14;
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    const ushort VK_LWIN = 0x5B;
    const ushort VK_LCONTROL = 0xA2;

    static IVirtualDesktopManager vdm = (IVirtualDesktopManager)new VirtualDesktopManager();

    public static uint Pid(IntPtr hwnd) { uint pid; GetWindowThreadProcessId(hwnd, out pid); return pid; }

    public static int Cloaked(IntPtr hwnd) { int v; return DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, out v, 4) == 0 ? v : 0; }

    // A window on another Windows desktop is cloaked by Windows, not by des-k: never judged.
    public static bool OnCurrentDesktop(IntPtr hwnd) { int on; return vdm.IsWindowOnCurrentVirtualDesktop(hwnd, out on) == 0 && on != 0; }

    // A process's full image path, or null when it can't be read.
    public static string Image(uint pid) {
        IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (h == IntPtr.Zero) return null;
        try {
            var sb = new StringBuilder(1024);
            uint size = (uint)sb.Capacity;
            return QueryFullProcessImageNameW(h, 0, sb, ref size) ? sb.ToString() : null;
        } finally { CloseHandle(h); }
    }

    public static string Exe(uint pid) {
        string path = Image(pid);
        return path == null ? "?" : System.IO.Path.GetFileName(path).ToLowerInvariant();
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

    // The screen holding most of the window is the one at (0, 0), the primary screen.
    public static bool OnPrimary(IntPtr hwnd) { return MonitorFromWindow(hwnd, 0) == MonitorFromPoint(new POINT(), 1); }

    // One process's top level windows, hidden by des-k or not: the stress run's helpers.
    public static List<IntPtr> Of(uint pid) {
        var list = new List<IntPtr>();
        EnumWindows((h, _) => {
            if (Pid(h) == pid && IsWindowVisible(h) && GetWindow(h, GW_OWNER) == IntPtr.Zero
                && ((long)GetWindowLongPtrW(h, GWL_EXSTYLE) & WS_EX_TOOLWINDOW) == 0) list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list;
    }

    static INPUT Key(ushort vk, bool up) {
        var k = new INPUT();
        k.type = 1; // INPUT_KEYBOARD
        k.u.ki.vk = vk;
        // KEYEVENTF_KEYUP, and KEYEVENTF_EXTENDEDKEY for the Windows key and the arrows, as des-k's own combo sends them.
        k.u.ki.flags = (up ? 2u : 0u) | (vk == VK_LCONTROL ? 0u : 1u);
        return k;
    }

    // Ctrl+Win and an arrow, pressed and let go as your hands would: no MAGIC tag, so des-k's hook hears it.
    public static void Combo(ushort arrow) {
        ushort[] keys = { VK_LWIN, VK_LCONTROL, arrow };
        var inputs = new INPUT[6];
        for (int i = 0; i < 3; i++) { inputs[i] = Key(keys[i], false); inputs[5 - i] = Key(keys[i], true); }
        if (SendInput(6, inputs, Marshal.SizeOf(typeof(INPUT))) != 6)
            throw new Exception("SendInput refused the keys, error " + Marshal.GetLastWin32Error());
    }

    // GDI objects (0) or USER objects (1) a process holds; 0 when they can't be read.
    public static uint Gui(uint pid, uint which) {
        IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (h == IntPtr.Zero) return 0;
        try { return GetGuiResources(h, which); } finally { CloseHandle(h); }
    }

    // ES_CONTINUOUS with the system and the display required, or ES_CONTINUOUS alone to let them sleep.
    public static void Awake(bool on) { SetThreadExecutionState(on ? 0x80000003u : 0x80000000u); }
}
'@

function Test-Admin {
    ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# The stress run (spec 0037). Each step presses one switch key and saves settings.toml once.
$period = 1200
$settingsFile = Join-Path $env:LOCALAPPDATA 'des-k\settings.toml'
$twiceAt = -1 # byte offset of twice_ms's three digits, once setup put 300 there
$helpers = New-Object System.Collections.Generic.List[System.Diagnostics.Process]
# Allowed growth from the warm sample to the end sample.
$limits = [ordered]@{ handles = 20; gdi = 10; user = 10; private = 2MB }

# The 10 plain windows a helper shows on the primary screen, then it waits to be stopped.
$helperScript = @'
Add-Type -AssemblyName System.Windows.Forms
$area = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
# A process's first shown window takes its start style, hidden here: a throwaway form takes it.
$first = New-Object System.Windows.Forms.Form
$first.Show()
$first.Dispose()
for ($i = 0; $i -lt 10; $i++) {
    $form = New-Object System.Windows.Forms.Form
    $form.Text = 'des-k stress'
    $form.StartPosition = 'Manual'
    $form.SetBounds($area.X + 60 + 40 * $i, $area.Y + 60 + 30 * $i, 420, 300)
    $form.Show()
}
[System.Windows.Forms.Application]::Run()
'@

# settings.toml's one twice_ms line: its text, and where its digits start in bytes.
function Find-Twice {
    $text = (New-Object System.Text.UTF8Encoding $false).GetString([IO.File]::ReadAllBytes($settingsFile))
    $found = [regex]::Matches($text, '(?m)^[ \t]*twice_ms[ \t]*=[ \t]*(\d+)')
    if ($found.Count -ne 1) { throw "stress: settings.toml needs one twice_ms = <number> line, it has $($found.Count)" }
    $digits = $found[0].Groups[1]
    [pscustomobject]@{ text = $text; index = $digits.Index; length = $digits.Length; at = [Text.Encoding]::UTF8.GetByteCount($text.Substring(0, $digits.Index)) }
}

# Changes the three digits in place: the file keeps its length, so des-k never reads half a file.
function Set-Twice([string]$value) {
    $fs = [IO.File]::Open($settingsFile, 'Open', 'ReadWrite', 'ReadWrite')
    try {
        $old = New-Object byte[] 4
        $fs.Position = $twiceAt
        $n = $fs.Read($old, 0, 4)
        $was = [Text.Encoding]::ASCII.GetString($old, 0, $n)
        if ($was -notmatch '^\d{3}(\D|$)') { throw 'stress: settings.toml changed during the run, twice_ms is no longer where it was' }
        if ($was.Substring(0, 3) -eq $value) { return }
        $fs.Position = $twiceAt
        $fs.Write([Text.Encoding]::ASCII.GetBytes($value), 0, 3)
    } finally { $fs.Dispose() }
}

function Get-Primary { @([DeskDrill]::Seen() | Where-Object { [DeskDrill]::OnPrimary($_) }) }

function Format-Windows($list) {
    $names = @($list | ForEach-Object { '{0} 0x{1:x}' -f [DeskDrill]::Exe([DeskDrill]::Pid($_)), [int64]$_ })
    if ($names.Count) { $names -join ', ' } else { 'none' }
}

# Stops the run unless the primary screen shows exactly these windows.
function Assert-Only($expected, [string]$when) {
    $on = @(Get-Primary)
    $extra = @($on | Where-Object { $expected -notcontains $_ })
    $missing = @($expected | Where-Object { $on -notcontains $_ })
    if ($extra.Count -or $missing.Count) {
        throw "stress: $when, the primary screen shows other windows ($(Format-Windows $extra)) and misses $($missing.Count) helper window(s)"
    }
}

function Start-Helper([string]$name) {
    $code = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($helperScript))
    $p = Start-Process -FilePath 'powershell.exe' -ArgumentList '-NoProfile', '-NonInteractive', '-EncodedCommand', $code -WindowStyle Hidden -PassThru
    $helpers.Add($p)
    $until = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 250
        $windows = @([DeskDrill]::Of([uint32]$p.Id))
    } while ($windows.Count -lt 10 -and (Get-Date) -lt $until)
    if ($windows.Count -ne 10) { throw "stress: helper $name showed $($windows.Count) of 10 windows in 30 s" }
    $windows
}

# Waits on the run's clock, never a fixed time after the work, so slow steps don't add up.
function Wait-Until($clock, [long]$ms) {
    while (($left = $ms - $clock.ElapsedMilliseconds) -gt 0) { Start-Sleep -Milliseconds ([Math]::Min($left, 200)) }
}

function Test-Cloaks($shown, $hidden) {
    foreach ($h in $shown) { if ([DeskDrill]::Cloaked($h) -ne 0) { return $false } }
    foreach ($h in $hidden) { if ([DeskDrill]::Cloaked($h) -eq 0) { return $false } }
    $true
}

function Get-Sample($step) {
    $p = Get-Process -Id $deskPid -ErrorAction SilentlyContinue
    if (-not $p -or $p.ProcessName -ne 'des-k') { return $null }
    [pscustomobject]@{
        step = $step; handles = $p.HandleCount; gdi = [DeskDrill]::Gui($deskPid, 0); user = [DeskDrill]::Gui($deskPid, 1)
        private = $p.PrivateMemorySize64; working = $p.WorkingSet64
    }
}

function Format-Sample($s) {
    'stress: {0}: handles {1}, GDI {2}, USER {3}, private {4:N1} MB, working set {5:N1} MB' -f $s.step, $s.handles, $s.gdi, $s.user, ($s.private / 1MB), ($s.working / 1MB)
}

# The last Quit line of this run: des-k.log, or des-k.old.log if the log rolled. A file not
# written since the run began belongs to another run.
function Read-Numbers([datetime]$since) {
    foreach ($name in 'des-k.log', 'des-k.old.log') {
        $file = Join-Path $env:LOCALAPPDATA "des-k\$name"
        if (-not (Test-Path $file) -or (Get-Item $file).LastWriteTime -lt $since) { continue }
        $hit = Select-String -Path $file -Pattern 'drill: stress switches=(\d+) reloads=(\d+) p50=(\S+) p95=(\S+) max=(\S+)' | Select-Object -Last 1
        if ($hit) {
            $g = $hit.Matches[0].Groups
            return [pscustomobject]@{ switches = [int64]$g[1].Value; reloads = [int64]$g[2].Value; p50 = $g[3].Value; p95 = $g[4].Value; max = $g[5].Value }
        }
    }
    $null
}

function Get-Commit {
    try {
        $c = git -C $PSScriptRoot rev-parse --short HEAD 2>$null
        if (-not $c) { return '?' }
        if (git -C $PSScriptRoot status --porcelain 2>$null) { "$c-dirty" } else { "$c" }
    } catch { '?' }
}

function Format-Growth([string]$key, $warm, $end) {
    if (-not $warm -or -not $end) { return '-' }
    $a = $warm.$key; $b = $end.$key
    if ($key -eq 'private') { 'warm {0:N1} MB, end {1:N1} MB, {2:+0.0;-0.0;0.0} MB' -f ($a / 1MB), ($b / 1MB), (($b - $a) / 1MB) }
    else { 'warm {0}, end {1}, {2:+0;-0;0}' -f $a, $b, ($b - $a) }
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
    'stress' {
        $started = Get-Date
        # Real pixels, so the primary screen's center is where the mouse really goes.
        [void][DeskDrill]::SetProcessDPIAware()
        Add-Type -AssemblyName System.Windows.Forms
        $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $cx = $bounds.X + [int][Math]::Floor($bounds.Width / 2)
        $cy = $bounds.Y + [int][Math]::Floor($bounds.Height / 2)

        # Checks before anything changes (AC-3).
        # The stress build runs as admin: reading its counters and CPU time needs an admin terminal too.
        if (-not (Test-Admin)) { throw 'stress: run it from an admin terminal, the one you started the stress build from' }
        $running = @(Get-Process -Name 'des-k' -ErrorAction SilentlyContinue)
        if ($running.Count -ne 1) { throw "stress: $($running.Count) des-k running, start exactly one: target\stress\release\des-k.exe" }
        $deskPid = [uint32]$running[0].Id
        $image = [DeskDrill]::Image($deskPid)
        if (-not $image -or -not $image.EndsWith('target\stress\release\des-k.exe', [StringComparison]::OrdinalIgnoreCase)) {
            throw 'stress: the des-k running is not the stress build. Build it (cargo build --release --features drill --target-dir target\stress), Quit this one, and start target\stress\release\des-k.exe from an admin terminal'
        }
        [void](Find-Twice)
        $busy = @(Get-Primary)
        if ($busy.Count) { throw "stress: the primary screen must show no app window. Move these to another screen or close them: $(Format-Windows $busy)" }
        $elsewhere = @([DeskDrill]::Seen()).Count
        $commit = Get-Commit

        $finished = $false; $stopped = $false; $quit = $false
        $done = 0; $bad = 0; $firstBad = 0
        $warm = $null; $end = $null; $idle = $null; $numbers = $null
        $why = New-Object System.Collections.Generic.List[string]
        try {
            # Setup (AC-4): twice_ms at 300, helper A on workspace A, then next, then helper B.
            $t = Find-Twice
            if ($t.length -ne 3) {
                $text = $t.text.Substring(0, $t.index) + '300' + $t.text.Substring($t.index + $t.length)
                [IO.File]::WriteAllText($settingsFile, $text, (New-Object System.Text.UTF8Encoding $false))
                $t = Find-Twice
            }
            $twiceAt = $t.at
            Set-Twice '300'
            Start-Sleep -Seconds 2
            $a = Start-Helper 'A'
            Assert-Only $a 'with helper A open'
            [void][DeskDrill]::SetCursorPos($cx, $cy)
            [DeskDrill]::Combo(0x27) # next
            Start-Sleep -Milliseconds $period
            $still = @($a | Where-Object { [DeskDrill]::Cloaked($_) -eq 0 })
            if ($still.Count) { throw "stress: next left $($still.Count) of helper A's 10 windows in view. Is next still Ctrl+Win+Right, and does des-k hear it?" }
            Assert-Only @() 'after next'
            $b = Start-Helper 'B'
            Assert-Only $b 'with helper B open'
            'stress: set up with 20 helper windows. {0} steps of {1} s, then 6 minutes of rest and idle: about {2:N1} hours. Ctrl+C stops it.' -f $Steps, ($period / 1000), (($Steps * $period / 1000 + 360) / 3600)

            # The steps (AC-5) and the samples (AC-6).
            [DeskDrill]::Awake($true)
            $warmAt = [int][Math]::Min(500, [Math]::Floor($Steps / 2))
            $clock = [Diagnostics.Stopwatch]::StartNew()
            for ($i = 1; $i -le $Steps; $i++) {
                Wait-Until $clock (($i - 1) * $period)
                [void][DeskDrill]::SetCursorPos($cx, $cy)
                $odd = $i % 2 -eq 1
                if ($odd) { [DeskDrill]::Combo(0x25); Set-Twice '301' } else { [DeskDrill]::Combo(0x27); Set-Twice '300' }
                Wait-Until $clock ($i * $period - 100)
                if ($odd) { $shown = $a; $hidden = $b } else { $shown = $b; $hidden = $a }
                if (-not (Test-Cloaks $shown $hidden)) {
                    if (-not $bad) { "stress: step $i left the helper windows wrong. It is counted and the run goes on, but it can only end INVALID" }
                    $bad++
                    if (-not $firstBad) { $firstBad = $i }
                }
                $done = $i
                if ($i -ge $warmAt -and ($i - $warmAt) % 500 -eq 0) {
                    $s = Get-Sample "step $i"
                    if (-not $s) { $stopped = $true; break }
                    if ($i -eq $warmAt) { $warm = $s }
                    Format-Sample $s
                }
            }

            # Rest, end sample and idle CPU (AC-6, AC-7).
            if (-not $stopped) {
                Wait-Until $clock ($Steps * $period)
                'stress: steps done, resting 60 s before the end sample'
                Start-Sleep -Seconds 60
                $end = Get-Sample 'end sample'
                if ($end) { Format-Sample $end } else { $stopped = $true }
            }
            if (-not $stopped) {
                'stress: idle CPU over the next 5 minutes, leave the PC alone'
                $cpu = (Get-Process -Id $deskPid).TotalProcessorTime
                $idleClock = [Diagnostics.Stopwatch]::StartNew()
                Start-Sleep -Seconds 300
                $p = Get-Process -Id $deskPid -ErrorAction SilentlyContinue
                if ($p) { $idle = ($p.TotalProcessorTime - $cpu).TotalMilliseconds / $idleClock.ElapsedMilliseconds * 100 } else { $stopped = $true }
            }

            # Finish (AC-8): Quit des-k, read its line, close the helpers.
            if (-not $stopped) {
                Start-Process -FilePath $image -ArgumentList '--quit' -WindowStyle Hidden -Wait
                $until = (Get-Date).AddSeconds(30)
                while ((Get-Process -Id $deskPid -ErrorAction SilentlyContinue) -and (Get-Date) -lt $until) { Start-Sleep -Milliseconds 250 }
                $quit = -not (Get-Process -Id $deskPid -ErrorAction SilentlyContinue)
                if ($quit) { $numbers = Read-Numbers $started }
            }
            foreach ($p in $helpers) { try { $p.Kill() } catch {} }
            $finished = $true
        } finally {
            # Also on Ctrl+C, when nothing more may go to the output: only .NET calls here.
            $put = $true
            if ($twiceAt -ge 0) { try { Set-Twice '300' } catch { $put = $false } }
            foreach ($p in $helpers) { try { $p.Kill() } catch {} }
            [DeskDrill]::Awake($false)
            if (-not $finished) {
                $twice = if ($put) { 'twice_ms is 300 again' } else { 'twice_ms could not be put back, set it to 300 by hand' }
                [Console]::WriteLine("stress: stopped early. $twice, the helper windows are closed, and des-k still runs: Quit it when you like.")
            }
        }

        # The verdict (AC-8).
        if ($stopped) {
            $verdict = 'FAIL'; $why.Add('des-k stopped during the run')
        } elseif (-not $numbers) {
            $verdict = 'INVALID'
            if ($quit) { $why.Add('no drill: stress line in the log. Is the running des-k the stress build?') } else { $why.Add('des-k did not quit within 30 s, so it wrote no numbers') }
        } else {
            # The setup's next is timed too; switches made before the run only add to the count.
            if ($numbers.switches -lt $Steps + 1) { $why.Add("switches=$($numbers.switches), below $($Steps + 1): some key presses never reached des-k") }
            if ($numbers.reloads -lt $Steps) { $why.Add("reloads=$($numbers.reloads), below $($Steps): some saves were never seen") }
            if ($bad) { $why.Add("$bad step(s) left the helper windows wrong, the first at step $firstBad") }
            if (-not $warm.gdi -or -not $warm.user) { $why.Add('GDI and USER objects could not be read') }
            if ($why.Count) {
                $verdict = 'INVALID'
            } else {
                foreach ($k in $limits.Keys) {
                    $grew = $end.$k - $warm.$k
                    if ($grew -gt $limits[$k]) {
                        if ($k -eq 'private') { $why.Add(('private bytes grew {0:N1} MB, more than 2 MB' -f ($grew / 1MB))) } else { $why.Add("$k grew $grew, more than $($limits[$k])") }
                    }
                }
                $verdict = if ($why.Count) { 'FAIL' } else { 'PASS' }
            }
        }

        # The result block: copy it into the record of this run.
        $nt = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
        $cpuName = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
        $ram = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB
        $took = (Get-Date) - $started
        $p50 = if ($numbers) { $numbers.p50 } else { '-' }
        $p95 = if ($numbers) { $numbers.p95 } else { '-' }
        $max = if ($numbers) { $numbers.max } else { '-' }
        $switches = if ($numbers) { $numbers.switches } else { '-' }
        $reloads = if ($numbers) { $numbers.reloads } else { '-' }
        ''
        "stress: $verdict"
        foreach ($w in $why) { "  why          $w" }
        "  commit       $commit"
        "  windows      $($nt.DisplayVersion), build $($nt.CurrentBuild).$($nt.UBR)"
        "  cpu          $cpuName"
        '  ram          {0:N1} GB' -f $ram
        "  screens      $([System.Windows.Forms.Screen]::AllScreens.Count)"
        "  app windows  20 on the primary screen (the helpers), $elsewhere on other screens"
        '  run          {0} of {1} steps, switches {2}, reloads {3}, cloak mismatches {4}, {5}h {6:00}m' -f $done, $Steps, $switches, $reloads, $bad, [Math]::Floor($took.TotalHours), $took.Minutes
        "  switch time  p50 $p50, p95 $p95, max $max"
        "  handles      $(Format-Growth 'handles' $warm $end)"
        "  gdi objects  $(Format-Growth 'gdi' $warm $end)"
        "  user objects $(Format-Growth 'user' $warm $end)"
        "  private      $(Format-Growth 'private' $warm $end)"
        if ($end) { '  working set  {0:N1} MB at the end sample' -f ($end.working / 1MB) } else { '  working set  -' }
        if ($null -ne $idle) { '  idle cpu     {0:N2} % of one core over 5 minutes' -f $idle } else { '  idle cpu     -' }
        exit $(switch ($verdict) { 'PASS' { 0 } 'FAIL' { 1 } default { 2 } })
    }
}
