# SoDeck memory speed test v1. Binary standard streams; no files/Add-Type/listeners.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($null -eq $script:InputStream) { throw 'missing bootstrap input' }
$script:OutputStream = [Console]::OpenStandardOutput()
$script:TxSequence = [uint32]0
$script:RxSequence = [uint32]0
$script:ControlBytes = [int]$script:BootstrapBytes
$script:Utf8 = [Text.UTF8Encoding]::new($false, $true)
$script:AckPhase = 0
$script:AckBytes = 0L
$script:AckClock = [Diagnostics.Stopwatch]::StartNew()
$script:ReadingData = $false

function Read-Exact([int]$length) {
    $buffer = [byte[]]::new($length)
    $offset = 0
    while ($offset -lt $length) {
        $pendingRead = $script:InputStream.ReadAsync($buffer, $offset, [Math]::Min(16384, $length - $offset))
        while (-not $pendingRead.Wait(50)) { Send-Progress }
        $count = $pendingRead.Result
        if ($count -eq 0) { throw [IO.EndOfStreamException]::new() }
        $offset += $count
        if ($script:ReadingData) { $script:AckBytes += $count }
        Send-Progress
    }
    return ,$buffer
}
function Send-Progress {
    if ($script:AckPhase -ne 0 -and $script:AckClock.Elapsed.TotalMilliseconds -ge 250) {
        Write-Frame 6 $script:AckPhase @{bytes=[string]$script:AckBytes}
        $script:AckClock.Restart()
    }
}
function Put-U32([byte[]]$buffer, [int]$offset, [uint32]$value) {
    $buffer[$offset] = [byte](($value -shr 24) -band 255)
    $buffer[$offset + 1] = [byte](($value -shr 16) -band 255)
    $buffer[$offset + 2] = [byte](($value -shr 8) -band 255)
    $buffer[$offset + 3] = [byte]($value -band 255)
}
function Get-U32([byte[]]$buffer, [int]$offset) {
    return [uint32](([uint64]$buffer[$offset] * 16777216) + ([uint64]$buffer[$offset + 1] * 65536) + ([uint64]$buffer[$offset + 2] * 256) + $buffer[$offset + 3])
}
function Write-Frame([int]$kind, [int]$phase, $value) {
    if ($kind -eq 5) { $payload = [byte[]]$value }
    else {
        $payload = $script:Utf8.GetBytes(($value | ConvertTo-Json -Compress -Depth 4))
        $script:ControlBytes += $payload.Length
        if ($payload.Length -gt 4096 -or $script:ControlBytes -gt 65536) { throw 'control budget' }
    }
    if ($payload.Length -gt 65536 -or $script:TxSequence -eq [uint32]::MaxValue) { throw 'frame budget' }
    $header = [byte[]]::new(16)
    $header[0] = 83; $header[1] = 68; $header[2] = 83; $header[3] = 49
    $header[4] = 1; $header[5] = [byte]$kind; $header[6] = [byte]$phase
    Put-U32 $header 8 $script:TxSequence
    Put-U32 $header 12 ([uint32]$payload.Length)
    $script:OutputStream.Write($header, 0, 16)
    $script:OutputStream.Write($payload, 0, $payload.Length)
    $script:OutputStream.Flush()
    $script:TxSequence++
}
function Read-Frame {
    $header = Read-Exact 16
    if ($header[0] -ne 83 -or $header[1] -ne 68 -or $header[2] -ne 83 -or $header[3] -ne 49 -or $header[4] -ne 1 -or $header[7] -ne 0 -or (Get-U32 $header 8) -ne $script:RxSequence) { throw 'header' }
    $kind = [int]$header[5]
    $phase = [int]$header[6]
    $length = Get-U32 $header 12
    if ($kind -lt 1 -or $kind -gt 9 -or $length -gt 65536 -or ($kind -ne 5 -and $length -gt 4096)) { throw 'length/type' }
    $script:RxSequence++
    $script:ReadingData = $kind -eq 5
    try { $payload = Read-Exact ([int]$length) } finally { $script:ReadingData = $false }
    if ($kind -ne 5) {
            $script:ControlBytes += $length
            if ($script:ControlBytes -gt 65536) { throw 'control budget' }
        $payload = ConvertFrom-Json -InputObject $script:Utf8.GetString($payload)
    }
    if ($kind -eq 9) { throw [IO.EndOfStreamException]::new() }
    return [pscustomobject]@{Kind=$kind; Phase=$phase; Payload=$payload}
}
function Hash-Hex($digest) { return [BitConverter]::ToString($digest.Hash).Replace('-', '').ToLowerInvariant() }
function Verify-Fields($value, [string[]]$fields) {
    $names = @($value.PSObject.Properties | ForEach-Object { $_.Name })
    if ($names.Count -ne $fields.Count) { throw 'fields' }
    foreach ($name in $names) { if ($fields -cnotcontains $name) { throw 'fields' } }
}

$random = [Security.Cryptography.RandomNumberGenerator]::Create()
try {
    Write-Frame 1 0 @{method='powershell-memory-v1'; protocol=1; hardDeadlineSeconds=60}
    $frame = Read-Frame
    if ($frame.Kind -ne 2 -or $frame.Phase -ne 0) { throw 'configuration' }
    $config = $frame.Payload
    Verify-Fields $config @('profile', 'directions')
    if ($config.profile -ceq 'standard') { $limit=268435456; $warm=2; $formal=10 }
    elseif ($config.profile -ceq 'save-data') { $limit=33554432; $warm=1; $formal=5 }
    else { throw 'profile' }
    $directions = @($config.directions)
    if (($directions -join ',') -cnotin @('upload','download','upload,download')) { throw 'directions' }
    Write-Frame 3 0 @{ready=$true}
    foreach ($direction in $directions) {
        $remaining = $limit
        foreach ($formalPhase in @(0,1)) {
            $phase = $(if ($direction -ceq 'upload') {1} else {3}) + $formalPhase
            $seconds = $(if ($formalPhase) {$formal} else {$warm})
            $frame = Read-Frame
            if ($frame.Kind -ne 4 -or $frame.Phase -ne $phase) { throw 'phase' }
            Verify-Fields $frame.Payload @()
            $digest = [Security.Cryptography.SHA256]::Create()
            try {
                $count = 0L
                $timer = [Diagnostics.Stopwatch]::StartNew()
                $phaseLimit = $(if ($formalPhase) {$remaining} else {[int]($limit/2)})
                if ($direction -ceq 'upload') {
                    $script:AckPhase=$phase; $script:AckBytes=0L; $script:AckClock.Restart()
                    while ($true) {
                        $frame = Read-Frame
                        if ($frame.Phase -ne $phase) { throw 'phase mismatch' }
                        if ($frame.Kind -eq 7) {
                            [void]$digest.TransformFinalBlock([byte[]]::new(0), 0, 0)
                            Verify-Fields $frame.Payload @('bytes', 'sha256')
                            if ($frame.Payload.bytes -cne [string]$count -or $frame.Payload.sha256 -cne (Hash-Hex $digest)) { throw 'verification' }
                            break
                        }
                        $payload = $frame.Payload
                        if ($frame.Kind -ne 5 -or $payload.Length -eq 0 -or $count + $payload.Length -gt $phaseLimit) { throw 'data budget' }
                        [void]$digest.TransformBlock($payload, 0, $payload.Length, $null, 0)
                        $count += $payload.Length
                    }
                } else {
                    while ($count -lt $phaseLimit -and $timer.Elapsed.TotalSeconds -lt $seconds) {
                        $payload = [byte[]]::new([Math]::Min(16384, $phaseLimit - $count))
                        $random.GetBytes($payload)
                        Write-Frame 5 $phase $payload
                        [void]$digest.TransformBlock($payload, 0, $payload.Length, $null, 0)
                        $count += $payload.Length
                    }
                    [void]$digest.TransformFinalBlock([byte[]]::new(0), 0, 0)
                }
                $hash = Hash-Hex $digest
                $script:AckPhase=0
                Write-Frame 8 $phase @{bytes=[string]$count; sha256=$hash; verified=$true}
                if ($direction -ceq 'download') {
                    $frame = Read-Frame
                    Verify-Fields $frame.Payload @('bytes', 'sha256')
                    if ($frame.Kind -ne 6 -or $frame.Phase -ne $phase -or $frame.Payload.bytes -cne [string]$count -or $frame.Payload.sha256 -cne $hash) { throw 'receipt' }
                }
                $remaining -= $count
            } finally { $digest.Dispose() }
        }
    }
    Write-Frame 8 0 @{complete=$true}
} catch [IO.EndOfStreamException] {
    # Closing stdin is cancellation, including a disconnected SSH client.
} catch {
    [Console]::Error.WriteLine('SODECK_SPEEDTEST_PROTOCOL_ERROR')
    exit 2
} finally {
    $random.Dispose()
}
