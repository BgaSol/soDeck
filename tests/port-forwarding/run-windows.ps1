# Tauri's unit-test EXE does not inherit the application Common Controls v6
# manifest. Embed it in this build artifact before running Windows native tests.
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$TestArgs)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
Push-Location $repo
try {
    $messages = & cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run --message-format=json
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $artifacts = $messages | ForEach-Object {
        try { $_ | ConvertFrom-Json } catch { }
    }
    $testExecutable = ($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.executable } | Select-Object -Last 1).executable
    if (-not $testExecutable) { throw 'Cargo did not return a test executable.' }
    $manifestTool = Get-ChildItem 'C:/Program Files (x86)/Windows Kits/10/bin' -Filter mt.exe -Recurse |
        Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $manifestTool) { throw 'Windows SDK manifest tool mt.exe is required.' }
    & $manifestTool.FullName -nologo -manifest (Join-Path $PSScriptRoot 'common-controls.manifest') "-outputresource:$testExecutable;#1"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if (-not $env:SSH_TEST_PYTHON) { $env:SSH_TEST_PYTHON = 'python' }
    & $testExecutable @TestArgs
    exit $LASTEXITCODE
} finally { Pop-Location }
