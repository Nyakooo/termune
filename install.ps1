# Windows PowerShell 5.1+ / PowerShell 7. Installs only into the current user directory.
& {
    $ErrorActionPreference = 'Stop'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    if ($env:OS -ne 'Windows_NT') { throw 'Use install.sh on Linux or macOS.' }
    $architecture = $env:PROCESSOR_ARCHITEW6432
    if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
    switch ($architecture.ToUpperInvariant()) {
        'AMD64' { $target = 'x86_64-pc-windows-msvc' }
        'ARM64' { $target = 'aarch64-pc-windows-msvc' }
        default { throw 'Supported CPU architectures: x86_64 and ARM64.' }
    }
    $repository = 'https://github.com/Nyakooo/termune'
    $version = $env:TERMUNE_VERSION
    if (-not $version) {
        $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/Nyakooo/termune/releases/latest' -TimeoutSec 120
        $version = $release.tag_name
        if (-not $version -or $release.draft -or $release.prerelease) { throw 'No published stable release is available.' }
    }
    $tag = if ($version.StartsWith('v')) { $version } else { "v$version" }
    if ($tag -notmatch '^v[0-9][a-zA-Z0-9.+-]*$') { throw "Invalid version: $tag" }
    $asset = "termune-$target.exe"
    $directory = $env:TERMUNE_INSTALL_DIR
    if (-not $directory) { $directory = Join-Path $env:LOCALAPPDATA 'Termune\bin' }
    $directory = [IO.Path]::GetFullPath($directory)
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
    $staged = $null
    New-Item -ItemType Directory -Path $temporary | Out-Null
    try {
        Write-Host "Downloading Termune $tag for $target..."
        $binary = Join-Path $temporary $asset
        $checksums = Join-Path $temporary 'SHA256SUMS'
        Invoke-WebRequest -UseBasicParsing -Uri "$repository/releases/download/$tag/$asset" -OutFile $binary -TimeoutSec 300
        Invoke-WebRequest -UseBasicParsing -Uri "$repository/releases/download/$tag/SHA256SUMS" -OutFile $checksums -TimeoutSec 120
        $entries = @(Get-Content -LiteralPath $checksums | Where-Object { $_ -match ('^[a-fA-F0-9]{64}\s+' + [regex]::Escape($asset) + '\s*$') })
        if ($entries.Count -ne 1) { throw 'Missing, duplicate or invalid checksum; nothing was installed.' }
        $size = (Get-Item -LiteralPath $binary).Length
        if ($size -lt 64 -or $size -gt 134217728) { throw 'Invalid binary size.' }
        $expected = ($entries[0] -split '\s+')[0]
        if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -ne $expected) { throw 'SHA256 mismatch; nothing was installed.' }
        $bytes = [IO.File]::ReadAllBytes($binary)
        if ($bytes[0] -ne 77 -or $bytes[1] -ne 90) { throw 'The release does not contain a Windows executable.' }
        $offset = [BitConverter]::ToUInt32($bytes, 60)
        if ([uint64]$offset + 6 -gt $bytes.Length) { throw 'Invalid PE header.' }
        $machine = if ($target.StartsWith('aarch64')) { 0xaa64 } else { 0x8664 }
        if ([BitConverter]::ToUInt32($bytes, $offset) -ne 0x4550 -or [BitConverter]::ToUInt16($bytes, $offset + 4) -ne $machine) { throw 'The executable architecture does not match this system.' }
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
        $destination = Join-Path $directory 'termune.exe'
        $staged = Join-Path $directory ('.termune-' + [guid]::NewGuid().ToString() + '.exe')
        Copy-Item -LiteralPath $binary -Destination $staged
        if (Test-Path -LiteralPath $destination) {
            [IO.File]::Replace($staged, $destination, [NullString]::Value)
        } else {
            [IO.File]::Move($staged, $destination)
        }
        $staged = $null
        if ($env:TERMUNE_NO_MODIFY_PATH -ne '1') {
            $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
            $pathEntries = @($userPath -split ';' | Where-Object { $_ })
            if ($pathEntries -notcontains $directory) {
                [Environment]::SetEnvironmentVariable('Path', (($pathEntries + $directory) -join ';'), 'User')
            }
            if (($env:Path -split ';') -notcontains $directory) { $env:Path = "$directory;$env:Path" }
        }
        Write-Host "Installed: $destination"
        Write-Host 'Run: termune (open a new terminal if needed).'
    } finally {
        if ($staged -and (Test-Path -LiteralPath $staged)) { Remove-Item -LiteralPath $staged -Force }
        Remove-Item -LiteralPath $temporary -Recurse -Force
    }
}
