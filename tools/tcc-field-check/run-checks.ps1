# Runs the hmn field check on a Windows box whose NVIDIA GPU is in TCC mode,
# and writes every command, its stdout, its stderr and its exit code to
# tcc-check.log beside this script. Windows PowerShell 5.1 compatible (the
# version Windows Server ships). Expects, beside it: hmn.exe, cudahold.exe,
# and fake\hmn.exe with fake\nvml.dll (see README.md).
#
#   powershell -ExecutionPolicy Bypass -File C:\tcc-kit\run-checks.ps1
#
# -SkipHolders leaves out section 3, the only one that allocates GPU memory.

param([switch]$SkipHolders)

$ErrorActionPreference = 'Stop'
$kit = $PSScriptRoot
$log = Join-Path $kit 'tcc-check.log'
$hmn = Join-Path $kit 'hmn.exe'
$fakeHmn = Join-Path $kit 'fake\hmn.exe'
$outFile = Join-Path $env:TEMP 'tcc-check-out.txt'
$errFile = Join-Path $env:TEMP 'tcc-check-err.txt'

foreach ($needed in @($hmn, (Join-Path $kit 'cudahold.exe'), $fakeHmn, (Join-Path $kit 'fake\nvml.dll'))) {
    if (-not (Test-Path $needed)) { throw "missing $needed" }
}

Set-Content -Path $log -Encoding UTF8 -Value "# hmn TCC field check, $(Get-Date -Format o)"

function Write-Log([string]$Text) {
    Add-Content -Path $log -Encoding UTF8 -Value $Text
    Write-Host $Text
}

function Write-Section([string]$Title) {
    Write-Log ''
    Write-Log "================ $Title ================"
}

# Run one native command, logging its stdout, stderr and exit code apart.
function Invoke-Logged([string]$Exe, [string[]]$Arguments = @()) {
    Write-Log ''
    Write-Log "### $([IO.Path]::GetFileName($Exe)) $($Arguments -join ' ')"
    $start = @{
        FilePath               = $Exe
        NoNewWindow            = $true
        Wait                   = $true
        PassThru               = $true
        RedirectStandardOutput = $outFile
        RedirectStandardError  = $errFile
    }
    if ($Arguments.Count -gt 0) { $start.ArgumentList = $Arguments }
    $p = Start-Process @start
    Write-Log "exit: $($p.ExitCode)"
    $out = Get-Content -Raw -Encoding UTF8 $outFile
    $err = Get-Content -Raw -Encoding UTF8 $errFile
    if ($out) { Write-Log "--- stdout`n$($out.TrimEnd())" }
    if ($err) { Write-Log "--- stderr`n$($err.TrimEnd())" }
}

# Start a GPU memory holder in the background; returns its process, or $null
# if it did not get its memory.
function Start-Holder([string]$Name, [int]$MiB) {
    $exe = Join-Path $kit "$Name.exe"
    if (-not (Test-Path $exe)) { Copy-Item (Join-Path $kit 'cudahold.exe') $exe }
    $hout = Join-Path $env:TEMP "$Name-out.txt"
    $herr = Join-Path $env:TEMP "$Name-err.txt"
    $p = Start-Process -FilePath $exe -ArgumentList "$MiB", '600' -NoNewWindow -PassThru `
        -RedirectStandardOutput $hout -RedirectStandardError $herr
    Start-Sleep -Seconds 5
    Write-Log ''
    Write-Log "### holder $Name ($MiB MiB): pid $($p.Id)"
    if ($p.HasExited) {
        Write-Log "holder exited with $($p.ExitCode): $(Get-Content -Raw $herr)"
        return $null
    }
    Write-Log "$(Get-Content -Raw $hout)".TrimEnd()
    return $p
}

$smi = (Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue).Source
if (-not $smi) { throw 'nvidia-smi.exe is not on PATH: install the NVIDIA driver first (README.md, step 3)' }

Write-Section '1. Machine'
Invoke-Logged $hmn @('--version')
Invoke-Logged $smi
Invoke-Logged $smi @('--query-gpu=index,name,pci.bus_id,driver_version,driver_model.current,driver_model.pending,memory.total,memory.used,memory.free', '--format=csv')
Write-Log ''
Write-Log '### Win32_VideoController'
Write-Log ((Get-CimInstance Win32_VideoController |
            Format-List Name, AdapterCompatibility, PNPDeviceID, Status, AdapterRAM | Out-String).Trim())
Write-Log ''
Write-Log "### OS: $((Get-CimInstance Win32_OperatingSystem).Caption) $([Environment]::OSVersion.Version)"

Write-Section '2. Idle GPU'
Invoke-Logged $smi @('--query-compute-apps=pid,process_name,used_memory', '--format=csv')
Invoke-Logged $hmn
Invoke-Logged $hmn @('--json')
Invoke-Logged $hmn @('ps')
Invoke-Logged $hmn @('ps', '--json')
Invoke-Logged $hmn @('ps', '--exit-status')
Invoke-Logged $hmn @('ps', '--device', '1')
Invoke-Logged $hmn @('fits', '1GiB')
Invoke-Logged $hmn @('fits', '64GiB')

if (-not $SkipHolders) {
    Write-Section '3. Two CUDA processes holding memory'
    $a = Start-Holder 'holder-a' 2048
    $b = Start-Holder 'holder-b' 1024
    Invoke-Logged $smi @('--query-compute-apps=pid,process_name,used_memory', '--format=csv')
    Invoke-Logged $smi
    Invoke-Logged $hmn
    Invoke-Logged $hmn @('ps')
    Invoke-Logged $hmn @('ps', '--json')
    Invoke-Logged $hmn @('ps', '--exit-status')
    Invoke-Logged $hmn @('ps', '--filter', 'holder-b')
    Invoke-Logged $hmn @('ps', '--min', '1500MiB')
    $pids = @()
    if ($a) { $pids += "$($a.Id)" }
    if ($b) { $pids += "$($b.Id)" }
    if ($pids.Count -gt 0) {
        Invoke-Logged $hmn (@('watch') + $pids + @('--interval', '1s', '--duration', '4s'))
        Invoke-Logged $hmn (@('watch') + $pids + @('--interval', '1s', '--duration', '2s', '--json'))
    }
    Invoke-Logged $hmn @('watch', '--interval', '1s', '--duration', '4s')
    Invoke-Logged $hmn @('watch', '--filter', 'holder', '--interval', '1s', '--duration', '3s')
    # A PID that holds nothing on the GPU: hmn watch names it by a direct lookup.
    Invoke-Logged $hmn @('watch', "$PID", '--interval', '1s', '--duration', '2s')
    foreach ($h in @($a, $b)) { if ($h -and -not $h.HasExited) { Stop-Process -Id $h.Id -Force } }
    Start-Sleep -Seconds 2
    Invoke-Logged $hmn @('ps')
    Invoke-Logged $hmn @('spill', '--', (Join-Path $kit 'cudahold.exe'), '512', '5')
}

Write-Section '4. Fake NVML beside hmn.exe (DXGI sees no NVIDIA adapter here)'
$fakeVars = 'FAKE_NVML_INIT', 'FAKE_NVML_COUNT', 'FAKE_NVML_DEVICES', 'FAKE_NVML_PROCS'
foreach ($case in @(
        @{ Label = 'INIT=18 (driver upgraded without a reboot): expect exit 2'; Var = 'FAKE_NVML_INIT'; Value = '18' },
        @{ Label = 'INIT=9 (driver not loaded): expect no GPU, exit 0'; Var = 'FAKE_NVML_INIT'; Value = '9' },
        @{ Label = 'INIT=28 (driver found no GPU): expect no GPU, exit 0'; Var = 'FAKE_NVML_INIT'; Value = '28' },
        @{ Label = 'one fake device, named by NVML since DXGI has none'; Var = 'FAKE_NVML_DEVICES'; Value = 'Fake Tesla' }
    )) {
    foreach ($v in $fakeVars) { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
    Set-Item "Env:$($case.Var)" $case.Value
    Write-Log ''
    Write-Log "---------- $($case.Label) ----------"
    Invoke-Logged $fakeHmn
    Invoke-Logged $fakeHmn @('ps')
}
foreach ($v in $fakeVars) { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }

Write-Section 'Done'
Write-Log "Log: $log"
