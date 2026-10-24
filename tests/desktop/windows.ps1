param([ValidateSet('list','close','show','capture','cursor','drag','move')][string]$Action='list',[Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$AppProcessId,[string]$Title='SoDeck v2',[string]$OutputPath,[int]$X,[int]$Y,[int]$StartX,[int]$StartY)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
# The harness supplies the PID of the executable it launched. Never select a
# window from the desktop by title alone, or operate on another application's PID.
$taskExecutable=[System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../target/debug/sodeck-desktop.exe'))
$taskProcess=Get-Process -Id $AppProcessId -ErrorAction Stop
if(-not [string]::Equals($taskProcess.Path,$taskExecutable,[StringComparison]::OrdinalIgnoreCase)){throw 'PID does not belong to this workspace test executable'}
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Collections.Generic; using System.Text; using System.Runtime.InteropServices;
public static class SoDeckNativeTest {
 public delegate bool EnumProc(IntPtr h, IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder text,int max);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect rect);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool GetClientRect(IntPtr h,out Rect rect);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool ClientToScreen(IntPtr h,ref Point point);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool SetWindowPos(IntPtr h,IntPtr insertAfter,int x,int y,int width,int height,uint flags);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int cmd);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint dx,uint dy,uint data,UIntPtr extra);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
 public struct Rect {public int left,top,right,bottom;}
 public struct Point {public int x,y;}
 public class Window {public long handle; public string title; public bool visible; public int x,y,width,height,clientX,clientY,clientWidth,clientHeight; public uint dpi;}
 public static Window[] List(uint process) {
  var list=new List<Window>();EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid!=process)return true;
   var text=new StringBuilder(512);GetWindowText(h,text,512);if(text.Length==0)return true;
   Rect r;GetWindowRect(h,out r);Rect client;var origin=new Point();
   if(!GetClientRect(h,out client)||!ClientToScreen(h,ref origin))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   var dpi=GetDpiForWindow(h);if(dpi==0)throw new InvalidOperationException("Owned window has no DPI");
   list.Add(new Window{handle=h.ToInt64(),title=text.ToString(),visible=IsWindowVisible(h),x=r.left,y=r.top,width=r.right-r.left,height=r.bottom-r.top,clientX=origin.x,clientY=origin.y,clientWidth=client.right-client.left,clientHeight=client.bottom-client.top,dpi=dpi});return true;},IntPtr.Zero);return list.ToArray();
 }
}
'@
[SoDeckNativeTest]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$taskWindows=@([SoDeckNativeTest]::List($AppProcessId))
if($Action -eq 'list') {ConvertTo-Json -InputObject $taskWindows -Compress;exit}
$taskWindow=$taskWindows | Where-Object title -eq $Title | Select-Object -First 1
if(-not $taskWindow){throw 'Requested test App window not found in owned process'}
$taskHandle=[IntPtr]$taskWindow.handle
switch($Action){
 'close' {[SoDeckNativeTest]::PostMessage($taskHandle,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null}
 'show' {[SoDeckNativeTest]::ShowWindow($taskHandle,9)|Out-Null;[SoDeckNativeTest]::SetForegroundWindow($taskHandle)|Out-Null}
 'move' {if(-not [SoDeckNativeTest]::SetWindowPos($taskHandle,[IntPtr]::Zero,$X,$Y,0,0,0x15)){throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error()))}}
 'cursor' {[SoDeckNativeTest]::SetCursorPos($X,$Y)|Out-Null}
 'drag' {
  [SoDeckNativeTest]::SetForegroundWindow($taskHandle)|Out-Null
  [SoDeckNativeTest]::SetCursorPos($StartX,$StartY)|Out-Null;Start-Sleep -Milliseconds 100
  [SoDeckNativeTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
  try {for($step=1;$step -le 20;$step++){[SoDeckNativeTest]::SetCursorPos([int]($StartX+($X-$StartX)*$step/20),[int]($StartY+($Y-$StartY)*$step/20))|Out-Null;Start-Sleep -Milliseconds 25}}
  finally{[SoDeckNativeTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
 }
 'capture' {
  if(-not $OutputPath){throw 'Capture output is required'}
  $taskImage=New-Object System.Drawing.Bitmap($taskWindow.width,$taskWindow.height)
  $taskGraphics=[System.Drawing.Graphics]::FromImage($taskImage);$taskDc=$taskGraphics.GetHdc()
  try {if(-not [SoDeckNativeTest]::PrintWindow($taskHandle,$taskDc,2)){throw 'PrintWindow failed'}}
  finally{$taskGraphics.ReleaseHdc($taskDc);$taskGraphics.Dispose()}
  try {$taskImage.Save($OutputPath,[System.Drawing.Imaging.ImageFormat]::Png)}finally{$taskImage.Dispose()}
 }
}
