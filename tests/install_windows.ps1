$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $root | Out-Null
$keys = @('TERMUNE_INSTALL_DIR', 'TERMUNE_VERSION', 'TERMUNE_NO_MODIFY_PATH', 'PROCESSOR_ARCHITECTURE', 'PROCESSOR_ARCHITEW6432', 'OS')
$original = @{}
foreach ($key in $keys) { $original[$key] = [Environment]::GetEnvironmentVariable($key, 'Process') }
$termuneInstallerFixture = @{ BadChecksum = $false; Asset = $null; Binary = $null; BadMachine = $false; NoRelease = $false }
function Invoke-RestMethod {
    param($Uri, $TimeoutSec)
    if ($termuneInstallerFixture.NoRelease) { throw 'Fixture: release unavailable' }
    return @{ tag_name = 'v0.1.1'; draft = $false; prerelease = $false }
}
function Invoke-WebRequest {
    param([switch]$UseBasicParsing, $Uri, $OutFile, $TimeoutSec)
    if ($Uri.EndsWith('/SHA256SUMS')) {
        $hash = if ($termuneInstallerFixture.BadChecksum) { '0' * 64 } else { (Get-FileHash $termuneInstallerFixture.Binary -Algorithm SHA256).Hash }
        "$hash  $($termuneInstallerFixture.Asset)" | Set-Content -LiteralPath $OutFile -Encoding ascii
    } elseif ($Uri.EndsWith('/' + $termuneInstallerFixture.Asset)) {
        Copy-Item -LiteralPath $termuneInstallerFixture.Binary -Destination $OutFile
    } else { throw "Unexpected URL: $Uri" }
}
try {
    $env:OS = 'Windows_NT'
    $env:TERMUNE_INSTALL_DIR = Join-Path $root 'installation with spaces'
    $env:TERMUNE_NO_MODIFY_PATH = '1'
    $installer = Join-Path $PSScriptRoot '../install.ps1'
    foreach ($architecture in @('AMD64', 'ARM64')) {
        $env:PROCESSOR_ARCHITECTURE = $architecture
        $env:PROCESSOR_ARCHITEW6432 = ''
        $target = if ($architecture -eq 'ARM64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
        $bytes = New-Object byte[] 128
        $bytes[0] = 77; $bytes[1] = 90; $bytes[60] = 64; $bytes[64] = 80; $bytes[65] = 69
        $machine = if ($architecture -eq 'ARM64') { [uint16]0xaa64 } else { [uint16]0x8664 }
        [Array]::Copy([BitConverter]::GetBytes($machine), 0, $bytes, 68, 2)
        $termuneInstallerFixture.Asset = "termune-$target.exe"
        $termuneInstallerFixture.Binary = Join-Path $root $termuneInstallerFixture.Asset
        [IO.File]::WriteAllBytes($termuneInstallerFixture.Binary, $bytes)
        Remove-Item Env:TERMUNE_VERSION -ErrorAction SilentlyContinue
        & $installer
        $destination = Join-Path $env:TERMUNE_INSTALL_DIR 'termune.exe'
        if ((Get-FileHash $destination).Hash -ne (Get-FileHash $termuneInstallerFixture.Binary).Hash) { throw 'Wrong installed content' }
        $env:TERMUNE_VERSION = '0.1.1'
        & $installer
        Invoke-Expression (Get-Content -LiteralPath $installer -Raw)
        # Simulate an emulated x86 host on native ARM64.
        if ($architecture -eq 'ARM64') {
            $env:PROCESSOR_ARCHITECTURE = 'AMD64'
            $env:PROCESSOR_ARCHITEW6432 = 'ARM64'
            & $installer
        }
    }
    [IO.File]::WriteAllText($destination, 'previous executable')
    $termuneInstallerFixture.BadChecksum = $true
    $rejected = $false
    try { & $installer } catch { $rejected = $true }
    if (-not $rejected) { throw 'Bad checksum accepted' }
    if ([IO.File]::ReadAllText($destination) -ne 'previous executable') { throw 'Old binary changed on failure' }
    $termuneInstallerFixture.BadChecksum = $false
    $env:TERMUNE_VERSION = '../../bad'
    $rejected = $false
    try { & $installer } catch { $rejected = $true }
    if (-not $rejected) { throw 'Invalid version accepted' }
    Remove-Item Env:TERMUNE_VERSION
    $termuneInstallerFixture.NoRelease = $true
    $rejected = $false
    try { & $installer } catch { $rejected = $true }
    if (-not $rejected) { throw 'Missing release accepted' }
    Write-Host 'PASS: Windows x64/ARM64, latest/pinned versions, file/IEX invocation, emulation, spaces and failure preservation'
} finally {
    foreach ($key in $keys) { [Environment]::SetEnvironmentVariable($key, $original[$key], 'Process') }
    Remove-Item -LiteralPath $root -Recurse -Force
}
