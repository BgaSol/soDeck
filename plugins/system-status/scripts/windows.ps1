$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
$r=@{schemaVersion=1;nonce='__NONCE__';errors=@{}}
function Source($name,$body) { try {$r[$name]=& $body} catch {$r.errors[$name]='数据源不可用：'+$_.Exception.GetType().Name} }
function Num($n) {if ($null -eq $n) {return $null}; return $n.ToString([Globalization.CultureInfo]::InvariantCulture)}
switch ('__GROUP__') {
 'identity' {
  Source os {$o=Get-CimInstance Win32_OperatingSystem -OperationTimeoutSec 3; @{hostname=$o.CSName;system=$o.Caption;version=$o.Version;totalBytes=(Num ([uint64]$o.TotalVisibleMemorySize*1024))}}
  Source cpu {,@(Get-CimInstance Win32_Processor -OperationTimeoutSec 3 | ForEach-Object {@{model=$_.Name;cores=$_.NumberOfLogicalProcessors;architecture=if ($null -ne $_.Architecture) {[int]$_.Architecture} else {$null}}})}
  Source device {$o=Get-CimInstance Win32_ComputerSystem -OperationTimeoutSec 3; @{model=$o.Manufacturer+' '+$o.Model}}
 }
 'dynamic' {
  Source cpu {$o=Get-CimInstance Win32_PerfRawData_PerfOS_Processor -Filter "Name='_Total'" -OperationTimeoutSec 3; @{idle=(Num $o.PercentProcessorTime);time=(Num $o.Timestamp_Sys100NS)}}
  Source memory {$o=Get-CimInstance Win32_PerfRawData_PerfOS_Memory -OperationTimeoutSec 3; @{availableBytes=(Num $o.AvailableBytes)}}
  Source network {@(Get-CimInstance Win32_PerfRawData_Tcpip_NetworkInterface -OperationTimeoutSec 3 | ForEach-Object {@{id=$_.Name;name=$_.Name;received=(Num $_.BytesReceivedPersec);sent=(Num $_.BytesSentPersec);time=(Num $_.Timestamp_PerfTime);frequency=(Num $_.Frequency_PerfTime);counterInstance=$true}})}
  Source disks {@(Get-CimInstance Win32_PerfRawData_PerfDisk_PhysicalDisk -OperationTimeoutSec 3 | Where-Object {$_.Name -ne '_Total'} | ForEach-Object {@{id=$_.Name;name=$_.Name;read=(Num $_.DiskReadBytesPersec);write=(Num $_.DiskWriteBytesPersec);time=(Num $_.Timestamp_PerfTime);frequency=(Num $_.Frequency_PerfTime)}})}
 }
 'topology' {
  Source mounts {,@(Get-CimInstance Win32_Volume -Filter 'DriveType=3' -OperationTimeoutSec 3 | Where-Object {$_.Name -match '^[A-Za-z]:\\'} | ForEach-Object {@{id=$_.DeviceID;path=if ($_.Name.Length -le 3) {$_.Name} else {$_.Name.TrimEnd('\')};kind=$_.FileSystem}})}
  Source interfaces {@(Get-CimInstance Win32_NetworkAdapterConfiguration -OperationTimeoutSec 3 | ForEach-Object {@{id=$_.SettingID;name=$_.Description;index=$_.InterfaceIndex;addresses=@($_.IPAddress);mac=$_.MACAddress;enabled=$_.IPEnabled}})}
 }
 'capacity' {
  Source filesystems {,@(Get-CimInstance Win32_Volume -Filter 'DriveType=3' -OperationTimeoutSec 3 | Where-Object {$_.Name -match '^[A-Za-z]:\\'} | ForEach-Object {@{id=$_.DeviceID;path=if ($_.Name.Length -le 3) {$_.Name} else {$_.Name.TrimEnd('\')};name=$_.Label;kind=$_.FileSystem;total=(Num $_.Capacity);available=(Num $_.FreeSpace)}})}
 }
 'uptime' {
  Source uptime {$o=Get-CimInstance Win32_PerfFormattedData_PerfOS_System -OperationTimeoutSec 3; @{seconds=(Num $o.SystemUpTime)}}
 }
}
[Console]::WriteLine(($r | ConvertTo-Json -Compress -Depth 6))
