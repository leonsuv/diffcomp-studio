# Run the shipped executable, verify CLI output and a live native document window.
param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$Session,
    [string]$OutputDirectory = "dist/windows/verification"
)
$ErrorActionPreference = "Stop"
$Executable = (Resolve-Path $Executable).Path
$Session = (Resolve-Path $Session).Path
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$OutputDirectory = (Resolve-Path $OutputDirectory).Path
$VersionOutput = Join-Path $OutputDirectory "version.txt"
$VersionErrors = Join-Path $OutputDirectory "version-stderr.txt"
$VersionProcess = Start-Process $Executable -ArgumentList '--version' -Wait -PassThru -RedirectStandardOutput $VersionOutput -RedirectStandardError $VersionErrors
if ($VersionProcess.ExitCode -ne 0) { throw "--version failed with exit code $($VersionProcess.ExitCode)" }
$Version = (Get-Content $VersionOutput -Raw).Trim()
if ($Version -notmatch '^DiffComp Studio \d+\.\d+\.\d+') { throw "Unexpected version output: $Version" }
$Stdout = Join-Path $OutputDirectory "startup-stdout.txt"
$Stderr = Join-Path $OutputDirectory "startup-stderr.txt"
$AppProcess = Start-Process $Executable -ArgumentList @('--session', ('"' + $Session + '"')) -PassThru -RedirectStandardOutput $Stdout -RedirectStandardError $Stderr
try {
    $Deadline = (Get-Date).AddSeconds(45)
    do {
        Start-Sleep -Milliseconds 500
        $AppProcess.Refresh()
        if ($AppProcess.HasExited) { throw "Application exited during startup: $($AppProcess.ExitCode). $(Get-Content $Stderr -Raw)" }
    } while ($AppProcess.MainWindowHandle -eq 0 -and (Get-Date) -lt $Deadline)
    if ($AppProcess.MainWindowHandle -eq 0) { throw "No native application window appeared" }
    Start-Sleep -Seconds 5
    $AppProcess.Refresh()
    if ($AppProcess.HasExited) { throw "Application exited after opening the document window" }
    if (-not $AppProcess.Responding) { throw "Application window is not responding" }
    $Report = [ordered]@{
        version = $Version
        operatingSystem = [Environment]::OSVersion.VersionString
        architecture = $env:PROCESSOR_ARCHITECTURE
        windowTitle = $AppProcess.MainWindowTitle
        windowHandleCreated = $true
        windowResponding = $AppProcess.Responding
        session = 'Engineering comparison demo'
        executableSha256 = (Get-FileHash $Executable -Algorithm SHA256).Hash
        checkedAtUtc = (Get-Date).ToUniversalTime().ToString('o')
    }
    $Report | ConvertTo-Json | Set-Content (Join-Path $OutputDirectory "windows-runtime.json")
    Write-Host ($Report | ConvertTo-Json)
} finally {
    if (-not $AppProcess.HasExited) {
        $null = $AppProcess.CloseMainWindow()
        if (-not $AppProcess.WaitForExit(5000)) { $AppProcess.Kill() }
    }
}
