param([ValidateSet('get','set')][string]$Action='get',[int]$Flags=-1,[string]$Scheme='')
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
Add-Type @'
using System;using System.Runtime.InteropServices;
public class SoDeckContrast {
 [StructLayout(LayoutKind.Sequential)] public struct HC {public uint Size;public uint Flags;public IntPtr Scheme;}
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern bool SystemParametersInfo(uint action,uint param,ref HC value,uint flags);
 public static HC Read(){HC h=new HC();h.Size=(uint)Marshal.SizeOf(h);if(!SystemParametersInfo(0x42,h.Size,ref h,0))throw new Exception("High contrast query failed");return h;}
 public static void Set(uint flags,string scheme){HC h=Read();h.Flags=flags;IntPtr own=Marshal.StringToHGlobalUni(scheme??"");h.Scheme=own;try{if(!SystemParametersInfo(0x43,h.Size,ref h,2))throw new Exception("High contrast change failed");}finally{Marshal.FreeHGlobal(own);}}
}
'@
if($Action -eq 'set'){if($Flags -lt 0){throw 'Exact captured flags required'};[SoDeckContrast]::Set($Flags,$Scheme)}
$value=[SoDeckContrast]::Read()
@{flags=$value.Flags;scheme=[System.Runtime.InteropServices.Marshal]::PtrToStringUni($value.Scheme)}|ConvertTo-Json -Compress
