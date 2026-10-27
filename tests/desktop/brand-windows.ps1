param([Parameter(Mandatory=$true)][int]$AppProcessId,[Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$taskExecutable=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../target/debug/sodeck-desktop.exe'))
$taskProcess=Get-Process -Id $AppProcessId
if(-not [string]::Equals($taskProcess.Path,$taskExecutable,[StringComparison]::OrdinalIgnoreCase)){throw 'Only the workspace test executable is allowed'}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;using System.Text;using System.Collections.Generic;using System.Runtime.InteropServices;
public static class SoDeckBrandTest {
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")]public static extern bool EnumWindows(EnumProc f,IntPtr p);
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")]public static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll",EntryPoint="GetClassLongPtrW")]public static extern IntPtr GetClassLongPtr(IntPtr h,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern IntPtr FindWindow(string c,string t);
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [StructLayout(LayoutKind.Sequential)]public struct Rect {public int left,top,right,bottom;}
 public class Entry{public long handle,icon;public string title;}
 public static Entry[] Icons(uint process){var list=new List<Entry>();EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid!=process)return true;var title=new StringBuilder(512);GetWindowText(h,title,512);if(title.Length==0)return true;var icon=SendMessage(h,0x7F,(IntPtr)1,IntPtr.Zero);if(icon==IntPtr.Zero)icon=SendMessage(h,0x7F,IntPtr.Zero,IntPtr.Zero);if(icon==IntPtr.Zero)icon=GetClassLongPtr(h,-14);if(icon==IntPtr.Zero)icon=GetClassLongPtr(h,-34);list.Add(new Entry{handle=h.ToInt64(),icon=icon.ToInt64(),title=title.ToString()});return true;},IntPtr.Zero);return list.ToArray();}
}
'@
[SoDeckBrandTest]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$taskEntries=@([SoDeckBrandTest]::Icons($AppProcessId))
foreach($taskEntry in $taskEntries){
 if($taskEntry.icon -eq 0){continue}
 $taskIcon=[Drawing.Icon]::FromHandle([IntPtr]$taskEntry.icon)
 $taskBitmap=$taskIcon.ToBitmap()
 try{$taskBitmap.Save((Join-Path $OutputDirectory ("window-"+$taskEntry.handle+'.png')),[Drawing.Imaging.ImageFormat]::Png)}finally{$taskBitmap.Dispose()}
}
$taskFileIcon=[Drawing.Icon]::ExtractAssociatedIcon($taskExecutable)
$taskBitmap=$taskFileIcon.ToBitmap()
try{$taskBitmap.Save((Join-Path $OutputDirectory 'executable.png'),[Drawing.Imaging.ImageFormat]::Png)}finally{$taskBitmap.Dispose();$taskFileIcon.Dispose()}
$taskTray=[SoDeckBrandTest]::FindWindow('Shell_TrayWnd',$null)
$taskRect=New-Object SoDeckBrandTest+Rect
if($taskTray -ne [IntPtr]::Zero -and [SoDeckBrandTest]::GetWindowRect($taskTray,[ref]$taskRect)){
 $taskBitmap=New-Object Drawing.Bitmap(($taskRect.right-$taskRect.left),($taskRect.bottom-$taskRect.top))
 $taskGraphics=[Drawing.Graphics]::FromImage($taskBitmap)
 try{$taskGraphics.CopyFromScreen($taskRect.left,$taskRect.top,0,0,$taskBitmap.Size);$taskBitmap.Save((Join-Path $OutputDirectory 'taskbar.png'),[Drawing.Imaging.ImageFormat]::Png)}finally{$taskGraphics.Dispose();$taskBitmap.Dispose()}
}
ConvertTo-Json -InputObject $taskEntries -Compress
