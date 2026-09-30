# Build on Windows; image/PDF processing uses Rust and requires no external DLLs.
$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $ProjectRoot
try {
    # Bundle the C runtime into the executable, avoiding a separate VC++ redistributable.
    $PreviousRustFlags = $env:RUSTFLAGS
    $env:RUSTFLAGS = (($PreviousRustFlags + " -C target-feature=+crt-static").Trim())
    cargo build --locked --release -p dc_app --bin diffcomp-studio
    if ($LASTEXITCODE -ne 0) { throw "Release build failed" }
    $Version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
    $Destination = Join-Path $ProjectRoot "dist\windows\DiffComp-Studio"
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item "target\release\diffcomp-studio.exe" $Destination -Force
    Copy-Item README.md $Destination -Force
    $VersionOutput = Join-Path $Destination "version.txt"
    $Process = Start-Process (Join-Path $Destination "diffcomp-studio.exe") -ArgumentList "--version" -Wait -PassThru -RedirectStandardOutput $VersionOutput
    if ($Process.ExitCode -ne 0) { throw "Binary verification failed" }
    if ((Get-Content $VersionOutput -Raw) -notmatch "DiffComp Studio $([regex]::Escape($Version))") { throw "Version mismatch" }
    Compress-Archive -Path "$Destination\*" -DestinationPath "dist\windows\DiffComp-Studio-$Version.zip" -Force
    Write-Host "Distribution ready: $Destination"
} finally { $env:RUSTFLAGS = $PreviousRustFlags; Pop-Location }
