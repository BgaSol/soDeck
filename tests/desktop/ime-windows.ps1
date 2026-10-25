param(
 [ValidateSet('focus','state','language','keys')][string]$Action='state',
 [Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$AppProcessId,
 [string]$Title='SoDeck v2', [string]$Keys, [string]$Layout='0000000008040804', [long]$WindowHandle=0
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$imeExecutable=[System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../target/debug/sodeck-desktop.exe'))
$imeProcess=Get-Process -Id $AppProcessId -ErrorAction Stop
if(-not [string]::Equals($imeProcess.Path,$imeExecutable,[StringComparison]::OrdinalIgnoreCase)){throw 'PID does not belong to this workspace test executable'}
Add-Type @'
using System; using System.Collections.Generic; using System.Text; using System.Runtime.InteropServices;
public static class SoDeckImeTest {
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder text,int max);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h,uint flags);
 [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint thread,ref GuiInfo info);
 [DllImport("user32.dll")] static extern IntPtr GetKeyboardLayout(uint thread);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr h,uint message,IntPtr w,IntPtr l);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 [DllImport("user32.dll",SetLastError=true)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
 [DllImport("imm32.dll")] static extern IntPtr ImmGetDefaultIMEWnd(IntPtr h);
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] struct GuiInfo {public uint size,flags;public IntPtr active,focus,capture,menuOwner,moveSize,caret;public Rect caretRect;}
 [StructLayout(LayoutKind.Sequential)] struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Sequential)] struct Mouse {public int x,y;public uint mouseData,flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit)] struct InputUnion {[FieldOffset(0)]public Keyboard keyboard;[FieldOffset(0)]public Mouse mouse;}
 [StructLayout(LayoutKind.Sequential)] struct Input {public uint type;public InputUnion data;}
 public class State {public long main,foreground,focus;public uint foregroundPid,thread,focusPid,focusThread;public string layout,focusLayout;public long? open,conversion;}
 public static IntPtr Window(uint process,string title) {IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid==process){var text=new StringBuilder(512);GetWindowText(h,text,512);if(text.ToString()==title)found=h;}return true;},IntPtr.Zero);if(found==IntPtr.Zero)throw new InvalidOperationException("Owned test window not found");return found;}
 public static State Read(IntPtr main,uint process,bool requireForeground) {
  var foreground=GetForegroundWindow();uint pid;uint thread=GetWindowThreadProcessId(foreground,out pid);
  if(requireForeground&&(pid!=process||foreground!=main))throw new InvalidOperationException("Foreground changed; no keys sent");
  var info=new GuiInfo{size=(uint)Marshal.SizeOf(typeof(GuiInfo))};if(!GetGUIThreadInfo(thread,ref info))throw new InvalidOperationException("Cannot read focused window");
  uint focusPid;uint focusThread=GetWindowThreadProcessId(info.focus,out focusPid);
  if(requireForeground&&(info.focus==IntPtr.Zero||GetAncestor(info.focus,2)!=main))throw new InvalidOperationException("Focus is outside owned test window; no keys sent");
  var state=new State{main=main.ToInt64(),foreground=foreground.ToInt64(),focus=info.focus.ToInt64(),foregroundPid=pid,thread=thread,focusPid=focusPid,focusThread=focusThread,layout=GetKeyboardLayout(thread).ToInt64().ToString("X16"),focusLayout=GetKeyboardLayout(focusThread).ToInt64().ToString("X16")};
  if(pid==process&&info.focus!=IntPtr.Zero&&GetAncestor(info.focus,2)==main){var ime=ImmGetDefaultIMEWnd(info.focus);if(ime!=IntPtr.Zero){UIntPtr result;if(SendMessageTimeout(ime,0x283,new IntPtr(5),IntPtr.Zero,2,300,out result)!=IntPtr.Zero)state.open=(long)result.ToUInt64();if(SendMessageTimeout(ime,0x283,new IntPtr(1),IntPtr.Zero,2,300,out result)!=IntPtr.Zero)state.conversion=(long)result.ToUInt64();}}
  return state;
 }
 public static void Press(IntPtr main,uint process,ushort key) {
  Read(main,process,true);
  var inputs=new[]{new Input{type=1,data=new InputUnion{keyboard=new Keyboard{key=key}}},new Input{type=1,data=new InputUnion{keyboard=new Keyboard{key=key,flags=2}}}};
  if(SendInput(2,inputs,Marshal.SizeOf(typeof(Input)))!=2)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
 }
}
'@
$imeWindow=if($WindowHandle -ne 0){[IntPtr]$WindowHandle}else{[SoDeckImeTest]::Window($AppProcessId,$Title)}
[uint32]$imeOwner=0
[SoDeckImeTest]::GetWindowThreadProcessId($imeWindow,[ref]$imeOwner)|Out-Null
if($imeOwner -ne $AppProcessId){throw 'IME target is not owned by this test App'}
if($Action -eq 'focus') {
 [SoDeckImeTest]::ShowWindow($imeWindow,9)|Out-Null
 [SoDeckImeTest]::SetForegroundWindow($imeWindow)|Out-Null
 Start-Sleep -Milliseconds 100
 [SoDeckImeTest]::Read($imeWindow,$AppProcessId,$true) | ConvertTo-Json -Compress
 exit
}
if($Action -eq 'state'){[SoDeckImeTest]::Read($imeWindow,$AppProcessId,$false)|ConvertTo-Json -Compress;exit}
$imeState=[SoDeckImeTest]::Read($imeWindow,$AppProcessId,$true)
if($Action -eq 'language') {
 if($Layout -notmatch '^[0-9a-fA-F]{8,16}$'){throw 'Invalid HKL'}
 if(-not [SoDeckImeTest]::PostMessage([IntPtr]$imeState.focus,0x50,[IntPtr]::Zero,[IntPtr]([Convert]::ToInt64($Layout,16)))){throw 'Owned-window language request failed'}
 Start-Sleep -Milliseconds 100
 [SoDeckImeTest]::Read($imeWindow,$AppProcessId,$true)|ConvertTo-Json -Compress
 exit
}
if($Keys -eq 'SPACE'){$imeCodes=@(32)}elseif($Keys -eq 'SHIFT'){$imeCodes=@(16)}elseif($Keys -eq 'ESC'){$imeCodes=@(27)}elseif($Keys -eq 'ENTER'){$imeCodes=@(13)}elseif($Keys -cmatch '^[a-z]{1,40}$'){$imeCodes=@($Keys.ToCharArray()|ForEach-Object{[int][char]::ToUpperInvariant($_)})}else{throw 'Only bounded ASCII letters or SPACE/SHIFT/ESC/ENTER are allowed'}
foreach($imeCode in $imeCodes){[SoDeckImeTest]::Press($imeWindow,$AppProcessId,[uint16]$imeCode);Start-Sleep -Milliseconds 45}
[SoDeckImeTest]::Read($imeWindow,$AppProcessId,$true)|ConvertTo-Json -Compress
