param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][ValidateSet('arrange', 'drag')][string]$Action,
    [Parameter(Mandatory = $true)][string]$Plan
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SoDeckTestPointer {
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct MonitorInfo { public int Size; public Rect Monitor, Work; public uint Flags; }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref Point point);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr window, uint flags);
    [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr monitor, ref MonitorInfo info);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr window, int x, int y, int width, int height, bool repaint);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr window);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint source, uint target, bool attach);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
}
'@
[void][SoDeckTestPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
$pointerPlan = $Plan | ConvertFrom-Json
$testProcess = Get-Process -Id $ProcessId
$testWindow = $testProcess.MainWindowHandle
$ownerProcess = [uint32]0
[void][SoDeckTestPointer]::GetWindowThreadProcessId($testWindow, [ref]$ownerProcess)
if ($testWindow -eq [IntPtr]::Zero -or $ownerProcess -ne $ProcessId) { throw 'Test process has no owned main window' }
if ($Action -eq 'arrange') {
    $monitor = New-Object SoDeckTestPointer+MonitorInfo
    $monitor.Size = [Runtime.InteropServices.Marshal]::SizeOf($monitor)
    if (-not [SoDeckTestPointer]::GetMonitorInfo([SoDeckTestPointer]::MonitorFromWindow($testWindow, 2), [ref]$monitor)) { throw 'Cannot read test monitor' }
    [void][SoDeckTestPointer]::ShowWindow($testWindow, 9)
    $nativeScale = [SoDeckTestPointer]::GetDpiForWindow($testWindow) / 96.0
    $testWidth = [Math]::Min([int]($pointerPlan.width * $nativeScale), $monitor.Work.Right - $monitor.Work.Left - 180)
    $testHeight = [Math]::Min([int]($pointerPlan.height * $nativeScale), $monitor.Work.Bottom - $monitor.Work.Top - 80)
    if (-not [SoDeckTestPointer]::MoveWindow($testWindow, $monitor.Work.Left + 40, $monitor.Work.Top + 40, $testWidth, $testHeight, $true)) { throw 'Cannot position test window' }
    [void][SoDeckTestPointer]::SetForegroundWindow($testWindow)
    Start-Sleep -Milliseconds 250
    $client = New-Object SoDeckTestPointer+Rect
    $outer = New-Object SoDeckTestPointer+Rect
    $origin = New-Object SoDeckTestPointer+Point
    if (-not [SoDeckTestPointer]::GetClientRect($testWindow, [ref]$client) -or -not [SoDeckTestPointer]::ClientToScreen($testWindow, [ref]$origin) -or -not [SoDeckTestPointer]::GetWindowRect($testWindow, [ref]$outer)) { throw 'Cannot read test window geometry' }
    @{ x = $origin.X; y = $origin.Y; width = $client.Right; height = $client.Bottom; right = $outer.Right; workRight = $monitor.Work.Right; scale = $nativeScale } | ConvertTo-Json -Compress
} else {
    $foregroundOwner = [uint32]0
    $foregroundThread = [SoDeckTestPointer]::GetWindowThreadProcessId([SoDeckTestPointer]::GetForegroundWindow(), [ref]$foregroundOwner)
    $helperThread = [SoDeckTestPointer]::GetCurrentThreadId()
    $attachedInput = $foregroundThread -ne $helperThread -and [SoDeckTestPointer]::AttachThreadInput($helperThread, $foregroundThread, $true)
    try {
        [void][SoDeckTestPointer]::BringWindowToTop($testWindow)
        [void][SoDeckTestPointer]::SetForegroundWindow($testWindow)
    } finally {
        if ($attachedInput) { [void][SoDeckTestPointer]::AttachThreadInput($helperThread, $foregroundThread, $false) }
    }
    [void][SoDeckTestPointer]::GetWindowThreadProcessId([SoDeckTestPointer]::GetForegroundWindow(), [ref]$foregroundOwner)
    if ($foregroundOwner -ne $ProcessId) { throw 'Test window could not receive native pointer input' }
    $savedCursor = New-Object SoDeckTestPointer+Point
    [void][SoDeckTestPointer]::GetCursorPos([ref]$savedCursor)
    try {
        [void][SoDeckTestPointer]::SetCursorPos([int]$pointerPlan.from.x, [int]$pointerPlan.from.y)
        Start-Sleep -Milliseconds 100
        [SoDeckTestPointer]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 150
        for ($step = 1; $step -le 20; $step++) {
            $testX = [int]($pointerPlan.from.x + ($pointerPlan.to.x - $pointerPlan.from.x) * $step / 20)
            $testY = [int]($pointerPlan.from.y + ($pointerPlan.to.y - $pointerPlan.from.y) * $step / 20)
            [void][SoDeckTestPointer]::SetCursorPos($testX, $testY)
            Start-Sleep -Milliseconds 40
        }
        Start-Sleep -Milliseconds 300
    } finally {
        [SoDeckTestPointer]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 100
        [void][SoDeckTestPointer]::SetCursorPos($savedCursor.X, $savedCursor.Y)
    }
}
