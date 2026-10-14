$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$folder = Join-Path $repo ('.work/native-crash-' + [guid]::NewGuid().ToString('N'))
$exe = Join-Path $repo 'build/native.exe'
$process = Start-Process -FilePath $exe -ArgumentList @('--test', '--data-dir', $folder, '--hold-ms', '30000') -WindowStyle Hidden -PassThru
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    $report = Join-Path $folder 'test-result.json'
    while (!(Test-Path -LiteralPath $report)) {
        if ($process.HasExited) { throw ('Native test exited ' + $process.ExitCode) }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Native test timed out' }
        Start-Sleep -Milliseconds 50
    }
    $result = Get-Content -Raw -LiteralPath $report | ConvertFrom-Json
    $before = [IO.File]::ReadAllBytes((Join-Path $folder 'native.txt'))
    $second = Start-Process -FilePath $exe -ArgumentList @('--probe','--data-dir',$folder) -WindowStyle Hidden -Wait -PassThru
    if ($second.ExitCode -ne 1) { throw 'Concurrent instance was not rejected' }
    Stop-Process -Id $process.Id -Force
    $process.WaitForExit()
    $probe = Start-Process -FilePath $exe -ArgumentList @('--probe','--data-dir',$folder) -WindowStyle Hidden -Wait -PassThru
    if ($probe.ExitCode -ne 0) { throw 'Reopen failed' }
    $after = [IO.File]::ReadAllBytes((Join-Path $folder 'native.txt'))
    if ([Convert]::ToBase64String($before) -ne [Convert]::ToBase64String($after)) { throw 'Draft changed after forced exit' }
    $loaded = Get-Content -Raw (Join-Path $folder 'probe-result.json') | ConvertFrom-Json
    if ($loaded.characters -ne $result.characters) { throw 'Loaded draft length differs' }
    $result | Add-Member force_kill_reopen $true
    $result | Add-Member concurrent_instance_rejected $true
    $result | ConvertTo-Json
    [IO.File]::WriteAllBytes((Join-Path $folder 'native.txt'), [byte[]]@(255))
    $bad = Start-Process -FilePath $exe -ArgumentList @('--probe','--data-dir',$folder) -WindowStyle Hidden -Wait -PassThru
    if ($bad.ExitCode -ne 1 -or [IO.File]::ReadAllBytes((Join-Path $folder 'native.txt'))[0] -ne 255) { throw 'Invalid UTF8 protection failed' }
} finally {
    if (!$process.HasExited) { Stop-Process -Id $process.Id -Force }
}
