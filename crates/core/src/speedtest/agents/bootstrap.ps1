# Fixed in-memory loader. Watchdog also bounds a stalled script bootstrap.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$watchdog = [PowerShell]::Create()
[void]$watchdog.AddScript('param($targetProcess) Start-Sleep -Seconds 60; [Diagnostics.Process]::GetProcessById($targetProcess).Kill()').AddArgument($PID)
$watchResult = $watchdog.BeginInvoke()
$runner = $null
try {
    $stdin = [Console]::OpenStandardInput()
    if ($PSVersionTable.PSVersion.Major -le 5) {
        # Framework ConsoleStream waits incorrectly on OpenSSH's overlapped pipe.
        # Wrap the inherited handle; neither wrapper owns/closes the OS handle.
        $field = $stdin.GetType().GetField('_handle', [Reflection.BindingFlags]'Instance,NonPublic')
        if ($null -eq $field) { throw 'unsupported standard input' }
        $handle = $field.GetValue($stdin)
        $pipeHandle = [Microsoft.Win32.SafeHandles.SafePipeHandle]::new($handle.DangerousGetHandle(), $false)
        $inputPipe = [IO.Pipes.AnonymousPipeClientStream]::new([IO.Pipes.PipeDirection]::In, $pipeHandle)
    } else { $inputPipe = $stdin }
    $buffer = [byte[]]::new(__SOURCE_BYTES__)
    $offset = 0
    while ($offset -lt $buffer.Length) {
        $count = $inputPipe.Read($buffer, $offset, $buffer.Length - $offset)
        if ($count -eq 0) { throw 'bootstrap eof' }
        $offset += $count
    }
    $runner = [PowerShell]::Create()
    [void]$runner.AddScript(('param($sourceInput,$sourceSize);$script:InputStream=$sourceInput;$script:BootstrapBytes=$sourceSize;' + [Text.Encoding]::UTF8.GetString($buffer))).AddArgument($inputPipe).AddArgument($buffer.Length)
    [void]$runner.Invoke()
    if ($runner.HadErrors) { throw 'agent failed' }
} catch {
    [Console]::Error.WriteLine('SODECK_SPEEDTEST_AGENT_ERROR')
    exit 2
} finally {
    if ($null -ne $runner) { $runner.Dispose() }
    $watchdog.Stop()
    $watchdog.Dispose()
}
