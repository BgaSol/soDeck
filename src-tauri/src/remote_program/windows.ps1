$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$WarningPreference = 'SilentlyContinue'
$c = '__CONFIG__' | ConvertFrom-Json
$o = [Console]::OpenStandardOutput()
$e = [Console]::OpenStandardError()
$p = $null
$mainStarted = $false

function frame($ok, $index, $message) {
    $value = @{v=1; started=$ok}
    if (-not $ok) {
        if ($index) { $value.checkFailure = @{index=$index; message='Program identity check failed'} }
        else { $value.error = @{message=$message.Substring(0, [Math]::Min(128, $message.Length))} }
    }
    $b = [Text.Encoding]::UTF8.GetBytes(($value | ConvertTo-Json -Compress -Depth 4))
    $h = [Text.Encoding]::ASCII.GetBytes("SDP1 $($b.Length)`n")
    $e.Write($h, 0, $h.Length)
    $e.Write($b, 0, $b.Length)
    $e.Flush()
}

function child($line, $redirect) {
    $s = [Diagnostics.ProcessStartInfo]::new()
    $s.FileName = $c.file
    $s.Arguments = $line
    $s.UseShellExecute = $false
    $s.RedirectStandardInput = $redirect
    $s.RedirectStandardOutput = $redirect
    $s.RedirectStandardError = $redirect
    foreach ($name in $c.remove) { $s.EnvironmentVariables.Remove($name) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $s
    [void]$process.Start()
    if ($redirect) { $process.StandardInput.Close() }
    return $process
}

try {
    if (-not [IO.Path]::IsPathRooted($c.file)) {
        $c.file = @(Get-Command $c.file -CommandType Application -ErrorAction Stop)[0].Source
    }
    $index = 0
    foreach ($check in $c.checks) {
        $index++
        $p = child $check.line $true
        $streams = @($p.StandardOutput.BaseStream, $p.StandardError.BaseStream)
        $buffers = @([byte[]]::new(4096), [byte[]]::new(4096))
        $results = @([IO.MemoryStream]::new(), [IO.MemoryStream]::new())
        $tasks = @($streams[0].ReadAsync($buffers[0], 0, 4096), $streams[1].ReadAsync($buffers[1], 0, 4096))
        $total = 0
        try {
            while ($tasks[0] -or $tasks[1]) {
                for ($k=0; $k -lt 2; $k++) {
                    if ($tasks[$k] -and $tasks[$k].IsCompleted) {
                        $n = $tasks[$k].GetAwaiter().GetResult()
                        $total += $n
                        if ($total -gt 4096) { throw 'Check output limit' }
                        if ($n) {
                            $results[$k].Write($buffers[$k], 0, $n)
                            $tasks[$k] = $streams[$k].ReadAsync($buffers[$k], 0, 4096)
                        } else { $tasks[$k] = $null }
                    }
                }
                if ($tasks[0] -or $tasks[1]) { [Threading.Thread]::Sleep(1) }
            }
            $p.WaitForExit()
            $actual = [Text.Encoding]::UTF8.GetString($results[0].ToArray())
            if ($actual.EndsWith("`n")) {
                $actual = $actual.Substring(0, $actual.Length-1)
                if ($actual.EndsWith("`r")) { $actual = $actual.Substring(0, $actual.Length-1) }
            }
            if ($p.ExitCode -ne 0 -or $actual -cne $check.expected) { throw 'Check failed' }
        } catch {
            if (-not $p.HasExited) { $p.Kill() }
            if ($c.terminal) { throw 'Program identity check failed' }
            frame $false $index ''
            exit 1
        } finally {
            $p.Dispose()
            $p = $null
            $results[0].Dispose()
            $results[1].Dispose()
        }
    }
    $p = child $c.line (-not $c.terminal)
    $mainStarted = $true
    try {
        if (-not $c.terminal) {
            frame $true 0 ''
            $t0 = $p.StandardOutput.BaseStream.CopyToAsync($o)
            $t1 = $p.StandardError.BaseStream.CopyToAsync($e)
            while (-not $p.WaitForExit(100)) {
                if ($t0.IsFaulted -or $t1.IsFaulted) { throw 'Output transport closed' }
            }
            [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($t0, $t1))
        } else { $p.WaitForExit() }
        $code = $p.ExitCode
    } finally {
        if (-not $p.HasExited) { $p.Kill() }
        $p.Dispose()
        $p = $null
    }
    exit $code
} catch {
    if ($p -and -not $p.HasExited) { $p.Kill(); $p.Dispose() }
    if (-not $c.terminal -and -not $mainStarted) { frame $false 0 $_.Exception.Message }
    else {
        $bytes = [Text.Encoding]::UTF8.GetBytes($_.Exception.Message)
        $e.Write($bytes, 0, [Math]::Min(4096, $bytes.Length))
    }
    exit 1
}
