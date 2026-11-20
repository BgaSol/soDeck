# Runs in this interactive process only; never changes a saved profile or key binding.
$global:SoDeckSequence = 0
$global:SoDeckRunning = $false
$global:SoDeckOriginalPrompt = (Get-Command prompt -CommandType Function).ScriptBlock
function global:SoDeckEmit([string]$Kind, [string]$Result = '') {
    $tail = if ($Result -ne '') { ';' + $Result } else { '' }
    [Console]::Write(([char]27).ToString() + ']633;SODECK;@TOKEN@;reading;1;' + $Kind + ';' + $global:SoDeckSequence + $tail + [char]7)
}
try {
    if (-not (Get-Module PSReadLine)) { Import-Module PSReadLine -ErrorAction Stop }
    $global:SoDeckOriginalReadLine = (Get-Command PSConsoleHostReadLine -CommandType Function -ErrorAction Stop).ScriptBlock
    function global:PSConsoleHostReadLine {
        $line = & $global:SoDeckOriginalReadLine
        if (-not [string]::IsNullOrWhiteSpace($line)) {
            SoDeckEmit 'start'
            $global:SoDeckRunning = $true
        }
        return $line
    }
    function global:prompt {
        $succeeded = $?
        if ($global:SoDeckRunning) {
            # This is PowerShell pipeline success, not a guessed native process exit code.
            SoDeckEmit 'end' $(if ($succeeded) { '0' } else { '?' })
            $global:SoDeckRunning = $false
        }
        $global:SoDeckSequence++
        SoDeckEmit 'prompt'
        $text = & $global:SoDeckOriginalPrompt
        return [string]$text + ([char]27).ToString() + ']633;SODECK;@TOKEN@;reading;1;input;' + $global:SoDeckSequence + [char]7
    }
} catch {
    [Console]::Write(([char]27).ToString() + ']633;SODECK;@TOKEN@;unavailable;psreadline-unavailable' + [char]7)
}
