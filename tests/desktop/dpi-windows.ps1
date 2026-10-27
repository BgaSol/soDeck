param([ValidateSet('get','set')][string]$Action='get',[int]$Percent=0,[string]$Display='')
$ErrorActionPreference='Stop'
# Per-monitor DisplayConfig DPI. The invoking test must restore the captured
# percentage in finally; browser emulation is not evidence for this test.
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class SoDeckDpiTest {
 [StructLayout(LayoutKind.Sequential)] public struct Luid {public uint Low;public int High;}
 [StructLayout(LayoutKind.Sequential)] public struct Header {public int Type;public uint Size;public Luid Adapter;public uint Id;}
 [StructLayout(LayoutKind.Sequential)] public struct ScaleGet {public Header Header;public int Min;public int Current;public int Max;}
 [StructLayout(LayoutKind.Sequential)] public struct ScaleSet {public Header Header;public int Relative;}
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct SourceName {public Header Header;[MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)]public string Name;}
 [DllImport("user32.dll")] static extern int GetDisplayConfigBufferSizes(uint flags,out uint paths,out uint modes);
 [DllImport("user32.dll")] static extern int QueryDisplayConfig(uint flags,ref uint paths,IntPtr pathArray,ref uint modes,IntPtr modeArray,IntPtr topology);
 [DllImport("user32.dll")] static extern int DisplayConfigGetDeviceInfo(ref ScaleGet request);
 [DllImport("user32.dll")] static extern int DisplayConfigSetDeviceInfo(ref ScaleSet request);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int DisplayConfigGetDeviceInfo(ref SourceName request);
 public static ScaleGet Read(string display){
  uint pc,mc;int code=GetDisplayConfigBufferSizes(2,out pc,out mc);if(code!=0)throw new Exception("DisplayConfig sizes: "+code);
  IntPtr paths=Marshal.AllocHGlobal((int)pc*72),modes=Marshal.AllocHGlobal((int)mc*64);
  try {
   code=QueryDisplayConfig(2,ref pc,paths,ref mc,modes,IntPtr.Zero);if(code!=0)throw new Exception("DPI display query: "+code);
   ScaleGet get=new ScaleGet();get.Header.Type=-3;get.Header.Size=(uint)Marshal.SizeOf(typeof(ScaleGet));
   bool found=false;for(int i=0;i<pc;i++){
    SourceName source=new SourceName();source.Header.Type=1;source.Header.Size=(uint)Marshal.SizeOf(typeof(SourceName));
    source.Header.Adapter.Low=(uint)Marshal.ReadInt32(paths,i*72);source.Header.Adapter.High=Marshal.ReadInt32(paths,i*72+4);source.Header.Id=(uint)Marshal.ReadInt32(paths,i*72+8);
    if(DisplayConfigGetDeviceInfo(ref source)==0&&source.Name==display){get.Header.Adapter=source.Header.Adapter;get.Header.Id=source.Header.Id;found=true;break;}
   }if(!found)throw new Exception("Requested display was not found: "+display);
   code=DisplayConfigGetDeviceInfo(ref get);if(code!=0)throw new Exception("DPI query: "+code);return get;
  }finally{Marshal.FreeHGlobal(paths);Marshal.FreeHGlobal(modes);}
 }
 public static void Set(ScaleGet get,int relative){
  if(relative<get.Min||relative>get.Max)throw new Exception("Requested DPI is not supported");
  ScaleSet set=new ScaleSet();set.Header=get.Header;set.Header.Type=-4;set.Header.Size=(uint)Marshal.SizeOf(typeof(ScaleSet));set.Relative=relative;
  int code=DisplayConfigSetDeviceInfo(ref set);if(code!=0)throw new Exception("DPI change: "+code);
 }
}
'@
$dpiValues=@(100,125,150,175,200,225,250,300,350,400,450,500)
Add-Type -AssemblyName System.Windows.Forms
if(-not $Display){$Display=[System.Windows.Forms.Screen]::PrimaryScreen.DeviceName}
$dpiState=[SoDeckDpiTest]::Read($Display)
$recommendedIndex=-$dpiState.Min
$original=$dpiValues[$recommendedIndex+$dpiState.Current]
if($Action -eq 'set'){
 $dpiIndex=[Array]::IndexOf($dpiValues,$Percent)
 if($dpiIndex -lt 0){throw 'Unknown DPI percentage'}
 [SoDeckDpiTest]::Set($dpiState,$dpiIndex-$recommendedIndex)
 $dpiState=[SoDeckDpiTest]::Read($Display)
}
@{display=$Display;percent=$dpiValues[$recommendedIndex+$dpiState.Current];original=$original;min=$dpiState.Min;current=$dpiState.Current;max=$dpiState.Max}|ConvertTo-Json -Compress
