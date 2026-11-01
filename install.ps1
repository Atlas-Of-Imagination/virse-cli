# 归档只安装 virse.exe；skills 通过独立工具安装。
[CmdletBinding()]
param(
    [string]$Version = $env:VIRSE_VERSION,
    [string]$InstallDir = $env:VIRSE_INSTALL_DIR,
    [string]$Target = $env:VIRSE_TARGET,
    [string]$Repository = $env:VIRSE_REPO,
    [switch]$NoPathUpdate,
    [switch]$SkipSkills = ($env:VIRSE_SKIP_SKILLS -eq '1')
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Use install.sh on macOS/Linux.'
}
if ($PSVersionTable.PSVersion.Major -lt 6) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}
if ([string]::IsNullOrWhiteSpace($Version)) { $Version = 'latest' }
if ([string]::IsNullOrWhiteSpace($Repository)) { $Repository = 'Atlas-Of-Imagination/virse-cli' }
$useGh = $false
if (Get-Command gh -ErrorAction SilentlyContinue) {
    try {
        & gh auth status --hostname github.com *> $null
        $useGh = $LASTEXITCODE -eq 0
    } catch { $useGh = $false }
}
if ([string]::IsNullOrWhiteSpace($Target)) {
    try { $arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() }
    catch {
        $arch = $env:PROCESSOR_ARCHITEW6432
        if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    }
    switch -Regex ($arch) {
        '^(X64|AMD64)$' { $Target = 'x86_64-pc-windows-msvc' }
        '^(Arm64|aarch64)$' { $Target = 'aarch64-pc-windows-msvc' }
        default { throw "Unsupported CPU architecture: $arch" }
    }
}
if ($Target -notin @('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')) {
    throw "Unsupported Windows target: $Target"
}
if ($Version -eq 'latest') {
    if ($useGh) {
        $Version = & gh release view --repo $Repository --json tagName --jq .tagName
        if ($LASTEXITCODE -ne 0) { throw 'Could not resolve latest release with gh.' }
    } else {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/latest"
        $Version = $release.tag_name
    }
    if ([string]::IsNullOrWhiteSpace($Version)) { throw 'Could not resolve latest release.' }
}
if (-not $Version.StartsWith('v')) { $Version = "v$Version" }
if ($Version -notmatch '^v[0-9][A-Za-z0-9.+-]*$') { throw 'Invalid version tag.' }
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    if ($env:LOCALAPPDATA) { $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\virse\bin' }
    elseif ($HOME) { $InstallDir = Join-Path $HOME '.local\bin' }
    else { throw 'Pass -InstallDir to choose an installation directory.' }
}
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$stem = "virse-$Version-$Target"
$archive = "$stem.zip"
$baseUrl = "https://github.com/$Repository/releases/download/$Version"
$tempDir = Join-Path ([IO.Path]::GetTempPath()) ("virse-install-" + [Guid]::NewGuid().ToString('N'))
$pending = $null
New-Item -ItemType Directory -Path $tempDir | Out-Null

function Download-File([string]$Uri, [string]$OutFile) {
    Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $OutFile
}
function Contains-Directory([string]$PathList, [string]$Directory) {
    foreach ($entry in ($PathList -split ';')) {
        if ([string]::IsNullOrWhiteSpace($entry)) { continue }
        try { $full = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($entry)).TrimEnd('\', '/') }
        catch { continue }
        if ([string]::Equals($full, $Directory.TrimEnd('\', '/'), [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
    return $false
}
try {
    Write-Host "Installing virse $Version for $Target"
    $archivePath = Join-Path $tempDir $archive
    $checksumPath = Join-Path $tempDir 'checksum'
    if ($useGh) {
        & gh release download $Version --repo $Repository --pattern $archive --pattern "$stem.sha256" --dir $tempDir
        if ($LASTEXITCODE -ne 0) { throw 'Could not download release assets with gh.' }
        Move-Item -LiteralPath (Join-Path $tempDir "$stem.sha256") -Destination $checksumPath
    } else {
        Download-File "$baseUrl/$archive" $archivePath
        Download-File "$baseUrl/$stem.sha256" $checksumPath
    }
    $pattern = '^([a-fA-F0-9]{64}) [ *]' + [regex]::Escape($archive) + '$'
    $expected = $null
    foreach ($line in [IO.File]::ReadAllLines($checksumPath)) {
        if ($line -match $pattern) {
            if ($expected) { throw 'Duplicate archive checksum.' }
            $expected = $Matches[1]
        }
    }
    if (-not $expected) { throw 'Missing or invalid archive checksum.' }
    if ((Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash -ne $expected) {
        throw 'SHA-256 mismatch; existing installation unchanged.'
    }

    # 仅复制根目录中的 virse.exe；其他 ZIP 成员不解压、不安装。
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archivePath)
    try {
        $entries = @($zip.Entries | Where-Object { $_.FullName -ceq 'virse.exe' })
        if ($entries.Count -ne 1) { throw 'Archive must contain exactly one virse.exe.' }
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        $destination = Join-Path $InstallDir 'virse.exe'
        $pending = Join-Path $InstallDir ('.virse-install-' + [Guid]::NewGuid().ToString('N'))
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $pending)
    } finally { $zip.Dispose() }
    $binaryHash = (Get-FileHash -LiteralPath $pending -Algorithm SHA256).Hash
    # PowerShell 会把 $null 转成空字符串；这里需要真正的 null 字符串。
    if ([IO.File]::Exists($destination)) { [IO.File]::Replace($pending, $destination, [NullString]::Value) }
    else { [IO.File]::Move($pending, $destination) }
    $pending = $null
    if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $binaryHash) {
        throw 'Installed binary does not match the downloaded binary.'
    }

    if (-not $NoPathUpdate) {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if (-not (Contains-Directory $userPath $InstallDir)) {
            $newPath = if ([string]::IsNullOrWhiteSpace($userPath)) { $InstallDir } else { "$userPath;$InstallDir" }
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        }
        if (-not (Contains-Directory $env:Path $InstallDir)) { $env:Path = "$env:Path;$InstallDir" }
    }
    Write-Host "Installed virse to $destination"
    if ($NoPathUpdate) { Write-Host "Add $InstallDir to PATH if needed." }
    else { Write-Host 'Open a new terminal if virse is not available in your current shell.' }
    if (-not $SkipSkills) {
        $skillsSource = "https://github.com/$Repository/tree/$Version/skills"
        $retry = "npx -y skills add `"$skillsSource`" --skill '*' -y -g"
        $npxCommand = Get-Command npx.cmd -ErrorAction SilentlyContinue
        if (-not $npxCommand) { $npxCommand = Get-Command npx -ErrorAction SilentlyContinue }
        if ($npxCommand) {
            try {
                & $npxCommand -y skills add $skillsSource --skill '*' -y -g
                if ($LASTEXITCODE -ne 0) { throw "skills exited with code $LASTEXITCODE" }
                Write-Host "Installed/updated Virse skills for $Version."
            } catch {
                Write-Warning "Virse binary installed, but skills installation failed. Retry: $retry"
            } finally {
                # 可选 skills 步骤失败不改变二进制安装的成功状态。
                $global:LASTEXITCODE = 0
            }
        } else {
            Write-Warning "Virse binary installed. Install Node.js/npm to add skills, then run: $retry"
        }
    }
} finally {
    if ($pending -and (Test-Path -LiteralPath $pending)) { Remove-Item -LiteralPath $pending -Force }
    Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
}
