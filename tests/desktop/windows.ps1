param([ValidateSet('list','close','show','capture','screen','cursor','drag','move','click','menuClick','hit','resize','doubleClick','systemMenu','escape','menuKey','minimize')][string]$Action='list',[Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$AppProcessId,[string]$Executable='', [string]$Title='SoDeck v2',[string]$OutputPath,[int]$X,[int]$Y,[int]$StartX,[int]$StartY,[long]$WindowHandle=0,[string]$CancelDrag='false',[int]$Width,[int]$Height,[int]$KeyCode=27)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
# The harness supplies the PID of the executable it launched. Never select a
# window from the desktop by title alone, or operate on another application's PID.
$taskExecutable=if($Executable){[System.IO.Path]::GetFullPath($Executable)}else{[System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../target/debug/sodeck-desktop.exe'))}
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
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder text,int max);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect rect);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool GetClientRect(IntPtr h,out Rect rect);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool ClientToScreen(IntPtr h,ref Point point);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool SetWindowPos(IntPtr h,IntPtr insertAfter,int x,int y,int width,int height,uint flags);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int cmd);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
 [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint dx,uint dy,uint data,UIntPtr extra);
 [DllImport("user32.dll")] public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
 public struct Rect {public int left,top,right,bottom;}
 public struct Point {public int x,y;}
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h,uint command);
 [DllImport("user32.dll",EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h,int index);
 public class Window {public long owner; public bool toolWindow,topmost;public long handle; public string title; public bool visible,maximized,minimized; public int x,y,width,height,clientX,clientY,clientWidth,clientHeight; public uint dpi;}
 public static Window[] List(uint process) {
  var list=new List<Window>();EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid!=process)return true;
   var text=new StringBuilder(512);GetWindowText(h,text,512);if(text.Length==0)return true;
   Rect r;GetWindowRect(h,out r);Rect client;var origin=new Point();
   if(!GetClientRect(h,out client)||!ClientToScreen(h,ref origin))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   var dpi=GetDpiForWindow(h);if(dpi==0)throw new InvalidOperationException("Owned window has no DPI");
   list.Add(new Window{owner=GetWindow(h,4).ToInt64(),toolWindow=(GetWindowLongPtr(h,-20).ToInt64()&128)!=0,topmost=(GetWindowLongPtr(h,-20).ToInt64()&8)!=0,handle=h.ToInt64(),title=text.ToString(),visible=IsWindowVisible(h),maximized=IsZoomed(h),minimized=IsIconic(h),x=r.left,y=r.top,width=r.right-r.left,height=r.bottom-r.top,clientX=origin.x,clientY=origin.y,clientWidth=client.right-client.left,clientHeight=client.bottom-client.top,dpi=dpi});return true;},IntPtr.Zero);return list.ToArray();
 }
}
'@
[SoDeckNativeTest]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$taskWindows=@([SoDeckNativeTest]::List($AppProcessId))
if($Action -eq 'list') {ConvertTo-Json -InputObject $taskWindows -Compress;exit}
$taskWindow=$taskWindows | Where-Object {if($WindowHandle -ne 0){$_.handle -eq $WindowHandle}else{$_.title -eq $Title}} | Select-Object -First 1
if(-not $taskWindow){throw 'Requested test App window not found in owned process'}
$taskHandle=[IntPtr]$taskWindow.handle
function Focus-TestWindow {
 [SoDeckNativeTest]::SetForegroundWindow($taskHandle)|Out-Null
 # Test windows can be overlapped after SetWindowPos; foreground activation
 # from the short-lived helper is restricted by Windows. Raise only our HWND.
 [SoDeckNativeTest]::SetWindowPos($taskHandle,[IntPtr](-1),0,0,0,0,0x13)|Out-Null
 [SoDeckNativeTest]::SetWindowPos($taskHandle,[IntPtr](-2),0,0,0,0,0x13)|Out-Null
}
switch($Action){
 'close' {[SoDeckNativeTest]::PostMessage($taskHandle,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null}
 'show' {[SoDeckNativeTest]::ShowWindow($taskHandle,9)|Out-Null;Focus-TestWindow}
 'move' {if(-not [SoDeckNativeTest]::SetWindowPos($taskHandle,[IntPtr]::Zero,$X,$Y,0,0,0x15)){throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error()))}}
 'resize' {if($Width -lt 100 -or $Height -lt 100){throw 'Invalid test size'};[SoDeckNativeTest]::SetWindowPos($taskHandle,[IntPtr]::Zero,0,0,$Width,$Height,0x16)|Out-Null}
 'doubleClick' {Focus-TestWindow;[SoDeckNativeTest]::SetCursorPos($X,$Y)|Out-Null;Start-Sleep -Milliseconds 100;for($i=0;$i -lt 2;$i++){[SoDeckNativeTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[SoDeckNativeTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 80}}
 'systemMenu' {Focus-TestWindow;[SoDeckNativeTest]::keybd_event(18,0,0,[UIntPtr]::Zero);try{[SoDeckNativeTest]::keybd_event(32,0,0,[UIntPtr]::Zero);[SoDeckNativeTest]::keybd_event(32,0,2,[UIntPtr]::Zero)}finally{[SoDeckNativeTest]::keybd_event(18,0,2,[UIntPtr]::Zero)}}
 'menuKey' {[SoDeckNativeTest]::keybd_event([byte]$KeyCode,0,0,[UIntPtr]::Zero);[SoDeckNativeTest]::keybd_event([byte]$KeyCode,0,2,[UIntPtr]::Zero)}
 'menuClick' {
  $point=New-Object SoDeckNativeTest+Point;$point.x=$X;$point.y=$Y
  $menuHandle=[SoDeckNativeTest]::WindowFromPoint($point);[uint32]$menuOwner=0
  [SoDeckNativeTest]::GetWindowThreadProcessId($menuHandle,[ref]$menuOwner)|Out-Null
  $menuClass=New-Object System.Text.StringBuilder 128
  [SoDeckNativeTest]::GetClassName($menuHandle,$menuClass,128)|Out-Null
  if($menuOwner -ne $AppProcessId -or $menuClass.ToString() -ne '#32768'){throw 'Point is not inside the owned native popup; no input sent'}
  [SoDeckNativeTest]::SetCursorPos($X,$Y)|Out-Null;Start-Sleep -Milliseconds 100
  [SoDeckNativeTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 60;[SoDeckNativeTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
 }
 'minimize' {[SoDeckNativeTest]::ShowWindow($taskHandle,6)|Out-Null}
 'escape' {Focus-TestWindow;[SoDeckNativeTest]::keybd_event(27,0,0,[UIntPtr]::Zero);[SoDeckNativeTest]::keybd_event(27,0,2,[UIntPtr]::Zero)}
 'cursor' {[SoDeckNativeTest]::SetCursorPos($X,$Y)|Out-Null;$taskPoint=New-Object SoDeckNativeTest+Point;[SoDeckNativeTest]::GetCursorPos([ref]$taskPoint)|Out-Null;$taskUnder=[SoDeckNativeTest]::WindowFromPoint($taskPoint);$taskRoot=[SoDeckNativeTest]::GetAncestor($taskUnder,2);[uint32]$taskUnderPid=0;[SoDeckNativeTest]::GetWindowThreadProcessId($taskUnder,[ref]$taskUnderPid)|Out-Null;@{x=$taskPoint.x;y=$taskPoint.y;handle=$taskUnder.ToInt64();root=$taskRoot.ToInt64();pid=$taskUnderPid;expectedPid=$AppProcessId;expectedRoot=$taskHandle.ToInt64()}|ConvertTo-Json -Compress}
 'hit' {[SoDeckNativeTest]::SendMessage($taskHandle,0x84,[IntPtr]::Zero,[IntPtr](($Y -shl 16) -bor ($X -band 65535))).ToInt64()}
 'click' {Focus-TestWindow;[SoDeckNativeTest]::SetCursorPos($X,$Y)|Out-Null;Start-Sleep -Milliseconds 100;[SoDeckNativeTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 60;[SoDeckNativeTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
 'screen' {
  if(-not $OutputPath){throw 'Capture output is required'}
  $taskImage=New-Object System.Drawing.Bitmap($taskWindow.width,$taskWindow.height)
  $taskGraphics=[System.Drawing.Graphics]::FromImage($taskImage)
  try{$taskGraphics.CopyFromScreen($taskWindow.x,$taskWindow.y,0,0,$taskImage.Size);$taskImage.Save($OutputPath,[System.Drawing.Imaging.ImageFormat]::Png)}finally{$taskGraphics.Dispose();$taskImage.Dispose()}
 }
 'drag' {
  Focus-TestWindow
  [SoDeckNativeTest]::SetCursorPos($StartX,$StartY)|Out-Null;Start-Sleep -Milliseconds 100
  $taskPoint=New-Object SoDeckNativeTest+Point
  [SoDeckNativeTest]::GetCursorPos([ref]$taskPoint)|Out-Null
  $taskHitRoot=[SoDeckNativeTest]::GetAncestor([SoDeckNativeTest]::WindowFromPoint($taskPoint),2)
  if($taskHitRoot -ne $taskHandle){throw "Drag origin hit $taskHitRoot instead of owned source $taskHandle at $($taskPoint.x),$($taskPoint.y)"}
  [SoDeckNativeTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
  try {for($step=1;$step -le 20;$step++){[SoDeckNativeTest]::SetCursorPos([int]($StartX+($X-$StartX)*$step/20),[int]($StartY+($Y-$StartY)*$step/20))|Out-Null;Start-Sleep -Milliseconds 25}}
  finally{
   if($CancelDrag -eq 'true'){[SoDeckNativeTest]::keybd_event(27,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 100;[SoDeckNativeTest]::keybd_event(27,0,2,[UIntPtr]::Zero)}
   [SoDeckNativeTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  }
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
