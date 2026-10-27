param([ValidateSet('launch','inspect','button','close','show','capture','stop')][string]$Action='inspect',[Parameter(Mandatory=$true)][string]$Package,[int]$AppProcessId=0,[string]$Name='',[string]$OutputPath='',[int]$ObserveMs=5000)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$taskBase=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../.local/dev'))+[IO.Path]::DirectorySeparatorChar
$taskPackage=[IO.Path]::GetFullPath($Package)
if(-not $taskPackage.StartsWith($taskBase,[StringComparison]::OrdinalIgnoreCase)){throw 'Package must be an isolated workspace delivery directory'}
$taskExe=Join-Path $taskPackage 'sodeck-desktop.exe'
if(-not (Test-Path -LiteralPath $taskExe -PathType Leaf)){throw 'Test executable is missing'}
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type @'
using System;using System.Text;using System.Collections.Generic;using System.Runtime.InteropServices;
public static class StartupGuiTest {
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")]public static extern bool EnumWindows(EnumProc proc,IntPtr p);
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")]public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetClassName(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")]public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")]public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int command);
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr h);
 public struct Rect{public int left,top,right,bottom;}
 public class Win{public long handle;public uint pid;public string title,kind;public bool visible;public int x,y,width,height;}
 public static Win[] Windows(uint pid){var wins=new List<Win>();EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner!=pid)return true;var title=new StringBuilder(512);var kind=new StringBuilder(128);GetWindowText(h,title,512);GetClassName(h,kind,128);Rect r;GetWindowRect(h,out r);wins.Add(new Win{handle=h.ToInt64(),pid=owner,title=title.ToString(),kind=kind.ToString(),visible=IsWindowVisible(h),x=r.left,y=r.top,width=r.right-r.left,height=r.bottom-r.top});return true;},IntPtr.Zero);return wins.ToArray();}
}
'@
[StartupGuiTest]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
if($Action -eq 'launch'){
 $taskBefore=@(Get-Process | Where-Object {$_.Path -eq $taskExe} | Select-Object -ExpandProperty Id)
 # Explorer's ShellExecute path, with unrelated cwd and inherited stale overrides.
 $env:SODECK_DEV_MODE='1';$env:SODECK_DATA_DIR=Join-Path $taskPackage 'SHOULD-NOT-EXIST';$env:SODECK_PLUGIN_PACKAGE='missing-stale-package';$env:SODECK_SHELL_DEBUG_PORT='9989';$env:SODECK_WEBVIEW_ARGS='--remote-debugging-port=9988'
 $taskStart=New-Object Diagnostics.ProcessStartInfo;$taskStart.FileName=$taskExe;$taskStart.WorkingDirectory=$env:TEMP;$taskStart.UseShellExecute=$true;$taskStart.WindowStyle='Hidden'
 $taskLaunched=[Diagnostics.Process]::Start($taskStart)
 $taskObserved=@();$taskWatch=[Diagnostics.Stopwatch]::StartNew()
 while($taskWatch.Elapsed.TotalMilliseconds -lt $ObserveMs){$taskObserved+=@([StartupGuiTest]::Windows($taskLaunched.Id)|Where-Object {$_.kind -eq 'ConsoleWindowClass'});Start-Sleep -Milliseconds 40}
 @{pid=$taskLaunched.Id;consoleWindows=$taskObserved;before=$taskBefore;exited=$taskLaunched.HasExited}|ConvertTo-Json -Depth 5 -Compress;exit
}
$taskProcess=Get-Process -Id $AppProcessId
if(-not [string]::Equals($taskProcess.Path,$taskExe,[StringComparison]::OrdinalIgnoreCase)){throw 'PID does not belong to the exact test package'}
$taskWindows=@([StartupGuiTest]::Windows($AppProcessId));$taskVisible=@($taskWindows|Where-Object {$_.visible -and $_.title})
if($Action -eq 'stop'){Stop-Process -Id $AppProcessId;exit}
if($Action -eq 'show'){$taskMain=$taskWindows|Where-Object {$_.kind -eq 'Tauri Window' -and $_.title -eq 'SoDeck v2'}|Select-Object -First 1;[StartupGuiTest]::ShowWindow([IntPtr]$taskMain.handle,9)|Out-Null;[StartupGuiTest]::SetForegroundWindow([IntPtr]$taskMain.handle)|Out-Null;exit}
if($Action -eq 'close'){foreach($taskWin in $taskVisible){[StartupGuiTest]::PostMessage([IntPtr]$taskWin.handle,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null};exit}
if($Action -eq 'capture'){
 if(-not $OutputPath){throw 'OutputPath is required'}
 $taskWin=$taskVisible|Select-Object -Last 1;$taskBitmap=New-Object Drawing.Bitmap($taskWin.width,$taskWin.height);$taskGraphics=[Drawing.Graphics]::FromImage($taskBitmap);$taskDc=$taskGraphics.GetHdc()
 try{[StartupGuiTest]::PrintWindow([IntPtr]$taskWin.handle,$taskDc,2)|Out-Null}finally{$taskGraphics.ReleaseHdc($taskDc)}
 try{$taskBitmap.Save($OutputPath,[Drawing.Imaging.ImageFormat]::Png)}finally{$taskGraphics.Dispose();$taskBitmap.Dispose()};exit
}
$taskElements=@()
foreach($taskWin in $taskVisible){
 $taskRoot=[Windows.Automation.AutomationElement]::FromHandle([IntPtr]$taskWin.handle)
 $taskChildren=$taskRoot.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 foreach($taskChild in $taskChildren){
  if($Action -eq 'button' -and $taskChild.Current.Name -eq $Name){
   $taskPattern=$null
   if($taskChild.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$taskPattern)){$taskPattern.Invoke();@{invoked=$Name}|ConvertTo-Json -Compress;exit}
   if($taskChild.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$taskPattern)){$taskPattern.Expand();@{expanded=$Name}|ConvertTo-Json -Compress;exit}
   if($taskChild.Current.NativeWindowHandle -ne 0){
    $taskControl=[IntPtr]$taskChild.Current.NativeWindowHandle;[uint32]$taskOwner=0;[StartupGuiTest]::GetWindowThreadProcessId($taskControl,[ref]$taskOwner)|Out-Null
    $taskClass=New-Object Text.StringBuilder 128;[StartupGuiTest]::GetClassName($taskControl,$taskClass,128)|Out-Null
    if($taskOwner -eq $AppProcessId -and $taskClass.ToString() -eq 'Button'){[StartupGuiTest]::PostMessage($taskControl,0xF5,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null;@{invoked=$Name}|ConvertTo-Json -Compress;exit}
   }
  }
  if($taskChild.Current.Name){$taskElements+=@{name=$taskChild.Current.Name;type=$taskChild.Current.ControlType.ProgrammaticName;enabled=$taskChild.Current.IsEnabled;handle=$taskChild.Current.NativeWindowHandle;patterns=@($taskChild.GetSupportedPatterns()|ForEach-Object {$_.ProgrammaticName})}}
 }
}
if($Action -eq 'button'){throw "Owned accessible action not found: $Name"}
$taskProcesses=Get-CimInstance Win32_Process;$taskIds=[Collections.Generic.HashSet[int]]::new();[void]$taskIds.Add($AppProcessId)
do{$taskAdded=$false;foreach($taskChild in $taskProcesses){if($taskIds.Contains([int]$taskChild.ParentProcessId)-and $taskIds.Add([int]$taskChild.ProcessId)){$taskAdded=$true}}}while($taskAdded)
$taskTree=@($taskProcesses|Where-Object {$taskIds.Contains([int]$_.ProcessId)}|Select-Object ProcessId,ParentProcessId,Name,CommandLine)
@{windows=$taskWindows;elements=$taskElements;processes=$taskTree}|ConvertTo-Json -Depth 6 -Compress
