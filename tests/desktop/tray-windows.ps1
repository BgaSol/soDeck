param([ValidateSet('inspect','left','right','double','key','menu','outside','capture','leftExit','exitBurst')][string]$Action='inspect',
 [Parameter(Mandatory=$true)][int]$AppProcessId,[Parameter(Mandatory=$true)][string]$Executable,
 [int]$Count=1,[int]$KeyCode=27,[int]$MenuIndex=0,[string]$OutputPath)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$taskRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$taskExe=[IO.Path]::GetFullPath($Executable)
if(-not $taskExe.StartsWith($taskRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Test executable must be inside this workspace'}
$taskProcess=Get-Process -Id $AppProcessId
if(-not [string]::Equals($taskProcess.Path,$taskExe,[StringComparison]::OrdinalIgnoreCase)){throw 'Unexpected test process'}
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName WindowsBase
Add-Type @'
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class TrayTest {
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p,IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point p);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint f);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 [DllImport("user32.dll")] public static extern void keybd_event(byte k,byte s,uint f,UIntPtr e);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr h);
 [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref IconId id,out Rect r);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr m,uint i,StringBuilder s,int n,uint f);
 [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr h,IntPtr m,uint i,out Rect r);
 public struct Point {public int x,y;}
 public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct IconId {public uint size;public IntPtr hwnd;public uint id;public Guid guid;}
 public class Win {public long handle;public string title,cls;public uint pid;public bool visible,minimized,maximized,enabled;public Rect rect;}
 public static Win[] List(uint pid) {var result=new List<Win>();EnumWindows((h,l)=>{uint p;GetWindowThreadProcessId(h,out p);if(p!=pid)return true;var s=new StringBuilder(512);GetWindowText(h,s,512);var c=new StringBuilder(128);GetClassName(h,c,128);Rect r;GetWindowRect(h,out r);result.Add(new Win{handle=h.ToInt64(),pid=p,title=s.ToString(),cls=c.ToString(),visible=IsWindowVisible(h),minimized=IsIconic(h),maximized=IsZoomed(h),enabled=IsWindowEnabled(h),rect=r});return true;},IntPtr.Zero);return result.ToArray();}
 public static string Class(IntPtr h){var s=new StringBuilder(128);GetClassName(h,s,128);return s.ToString();}
 public static Rect Icon(IntPtr hwnd){for(uint i=0;i<32;i++){var id=new IconId{size=(uint)Marshal.SizeOf(typeof(IconId)),hwnd=hwnd,id=i};Rect r;if(Shell_NotifyIconGetRect(ref id,out r)==0&&r.right>r.left)return r;}throw new Exception("No tray icon rectangle for test process");}
 public static void Click(Rect r,bool right,int count){var p=new Point{x=(r.left+r.right)/2,y=(r.top+r.bottom)/2};SetCursorPos(p.x,p.y);System.Threading.Thread.Sleep(100);for(int i=0;i<count;i++){mouse_event(right?8u:2u,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(35);mouse_event(right?16u:4u,0,0,0,UIntPtr.Zero);if(i+1<count)System.Threading.Thread.Sleep(90);}}
}
'@
[TrayTest]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
function Snapshot {
 $taskWindows=@([TrayTest]::List($AppProcessId))
 $taskTray=@($taskWindows|Where-Object {$_.cls -eq 'tray_icon_app'})
 $taskIcon=$null
 if($taskTray.Count -eq 1){try{$taskIcon=[TrayTest]::Icon([IntPtr]$taskTray[0].handle)}catch{if($Action -ne 'exitBurst'){throw}}}
 $taskMenus=@()
 foreach($taskMenu in @($taskWindows|Where-Object {$_.cls -eq '#32768' -and $_.visible})){
  $taskHm=[TrayTest]::SendMessage([IntPtr]$taskMenu.handle,0x1E1,[IntPtr]::Zero,[IntPtr]::Zero)
  $taskItems=@();for($taskI=0;$taskI -lt [TrayTest]::GetMenuItemCount($taskHm);$taskI++){$taskText=New-Object Text.StringBuilder 256;[TrayTest]::GetMenuString($taskHm,$taskI,$taskText,256,0x400)|Out-Null;$taskR=New-Object TrayTest+Rect;[TrayTest]::GetMenuItemRect([IntPtr]$taskMenu.handle,$taskHm,$taskI,[ref]$taskR)|Out-Null;$taskItems+=@{text=$taskText.ToString();rect=$taskR}}
  $taskMenus+=@{handle=$taskMenu.handle;items=$taskItems}
 }
 $taskHit=$null
 if($taskIcon){$taskAt=New-Object Windows.Point;$taskAt.X=($taskIcon.left+$taskIcon.right)/2;$taskAt.Y=($taskIcon.top+$taskIcon.bottom)/2;$taskElement=[Windows.Automation.AutomationElement]::FromPoint($taskAt);if($taskElement){$taskHit=@{name=$taskElement.Current.Name;automationId=$taskElement.Current.AutomationId;class=$taskElement.Current.ClassName}}}
 @{windows=$taskWindows;foreground=[TrayTest]::GetForegroundWindow().ToInt64();icon=$taskIcon;iconHit=$taskHit;menus=$taskMenus}
}
function Prepare-Icon($taskCurrent) {
 if(-not $taskCurrent.icon){throw 'Expected one owned tray icon'}
 $taskOriginal=$taskCurrent.icon
 $taskExpanded=$false
 # Windows 11 returns the chevron rectangle for a hidden icon. Its root HWND
 # is still Shell_TrayWnd, so checking the window class alone is insufficient.
 $taskBarCondition=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ClassNameProperty,'Shell_TrayWnd')
 $taskBar=[Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children,$taskBarCondition)
 $taskChevron=@()
 if($taskBar){
  $taskChevron=@($taskBar.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.AutomationId -eq 'NotificationChevron' -or $_.Current.Name -in @('显示隐藏的图标','Show hidden icons')}|Select-Object -First 1)
 }
 $taskPoint=New-Object TrayTest+Point;$taskPoint.x=($taskOriginal.left+$taskOriginal.right)/2;$taskPoint.y=($taskOriginal.top+$taskOriginal.bottom)/2
 if($taskChevron.Count -eq 1){
  $taskBounds=$taskChevron[0].Current.BoundingRectangle
  if($taskBounds.Contains($taskPoint.x,$taskPoint.y)){
   $taskRect=New-Object TrayTest+Rect;$taskRect.left=$taskBounds.Left;$taskRect.top=$taskBounds.Top;$taskRect.right=$taskBounds.Right;$taskRect.bottom=$taskBounds.Bottom
   [TrayTest]::Click($taskRect,$false,1);Start-Sleep -Milliseconds 250
   $taskCurrent=Snapshot;$taskExpanded=$true
   if(-not $taskCurrent.icon){throw 'Owned icon disappeared while expanding overflow'}
   $taskPoint.x=($taskCurrent.icon.left+$taskCurrent.icon.right)/2;$taskPoint.y=($taskCurrent.icon.top+$taskCurrent.icon.bottom)/2
   if($taskBounds.Contains($taskPoint.x,$taskPoint.y)){throw 'Owned icon still resolves to the notification chevron'}
  }
 }
 $taskClass=[TrayTest]::Class([TrayTest]::GetAncestor([TrayTest]::WindowFromPoint($taskPoint),2))
 if($taskClass -notin @('Shell_TrayWnd','Shell_SecondaryTrayWnd','NotifyIconOverflowWindow','TopLevelWindowForOverflowXamlIsland')){throw "Owned icon is obscured ($taskClass)"}
 $script:taskInputTarget=@{before=$taskOriginal;target=$taskCurrent.icon;surface=$taskClass;expandedOverflow=$taskExpanded}
 return $taskCurrent
}
$taskState=Snapshot
$taskRace=@()
$taskInputTarget=$null
if($Action -in @('leftExit','exitBurst')){
 if($Action -eq 'leftExit'){
  $taskState=Prepare-Icon $taskState
  [TrayTest]::Click($taskState.icon,$false,1)
  $taskState=Prepare-Icon (Snapshot)
  [TrayTest]::Click($taskState.icon,$true,1)
  for($taskTry=0;$taskTry -lt 60;$taskTry++){Start-Sleep -Milliseconds 10;$taskState=Snapshot;if($taskState.menus.Count -eq 1 -and $taskState.menus[0].items.Count -eq 2){break}}
 }
 if($taskState.menus.Count -ne 1 -or $taskState.menus[0].items.Count -ne 2 -or -not $taskState.menus[0].items[1].text.EndsWith('SoDeck')){throw ('Expected owned Exit menu item: '+($taskState|ConvertTo-Json -Depth 8 -Compress))}
 [TrayTest]::Click($taskState.menus[0].items[1].rect,$false,1)
 for($taskTry=0;$taskTry -lt 10;$taskTry++){
  $taskState=Snapshot;$taskRace+=$taskState
  if(-not $taskState.icon){break}
  if($Action -eq 'exitBurst'){[TrayTest]::Click($taskState.icon,$false,1)}
  Start-Sleep -Milliseconds 20
 }
}elseif($Action -in @('left','right','double')){
 $taskState=Prepare-Icon $taskState
 [TrayTest]::Click($taskState.icon,($Action -eq 'right'),$(if($Action -eq 'double'){2}else{$Count}))
}elseif($Action -eq 'key'){
 if($taskState.menus.Count -ne 1){throw 'No owned native tray menu for keyboard input'}
 [TrayTest]::keybd_event([byte]$KeyCode,0,0,[UIntPtr]::Zero);[TrayTest]::keybd_event([byte]$KeyCode,0,2,[UIntPtr]::Zero)
}elseif($Action -eq 'menu'){
 if($taskState.menus.Count -ne 1){throw 'No owned native tray menu'}
 [TrayTest]::Click($taskState.menus[0].items[$MenuIndex].rect,$false,1)
}elseif($Action -eq 'outside'){
 if($taskState.menus.Count -ne 1){throw 'No owned menu to dismiss'}
 # Click inside a visible test-owned secondary window, never another app.
 $taskOther=@($taskState.windows|Where-Object {$_.cls -eq 'Tauri Window' -and $_.visible -and -not $_.minimized})
 if($taskOther.Count -ne 1){throw 'Outside dismissal requires exactly one visible test window'}
 [TrayTest]::Click($taskOther[0].rect,$false,1)
}elseif($Action -eq 'capture'){
 if(-not $OutputPath){throw 'Output required'}
 $taskRect=$taskState.icon
 if($taskState.menus.Count){$taskRect=($taskState.windows|Where-Object {$_.handle -eq $taskState.menus[0].handle}).rect}
 $taskBitmap=New-Object Drawing.Bitmap(($taskRect.right-$taskRect.left),($taskRect.bottom-$taskRect.top))
 $taskGraphics=[Drawing.Graphics]::FromImage($taskBitmap)
 try{$taskGraphics.CopyFromScreen($taskRect.left,$taskRect.top,0,0,$taskBitmap.Size);$taskBitmap.Save($OutputPath,[Drawing.Imaging.ImageFormat]::Png)}finally{$taskGraphics.Dispose();$taskBitmap.Dispose()}
}
Start-Sleep -Milliseconds 150
$taskFinal=Snapshot
if($taskInputTarget){$taskFinal.inputTarget=$taskInputTarget}
if($taskRace.Count){$taskFinal.raceSamples=$taskRace}
$taskFinal|ConvertTo-Json -Depth 10 -Compress
