# Installs the latest Nez-Code release for Windows, then offers to set up
# Ollama with a model sized for this machine.
#
#   irm https://github.com/NezPifs/Nez-Code/releases/latest/download/install.ps1 | iex
#
# Environment:
#   NEZ_VERSION          release tag to install (default: latest)
#   NEZ_INSTALL_DIR      where to put nez.exe (default: %USERPROFILE%\.nez\bin)
#   NEZ_NON_INTERACTIVE  1 = answer every question with its default
#   NEZ_SKIP_OLLAMA      1 = do not touch Ollama or models
#   GITHUB_TOKEN         token for private repositories or rate limits

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = 'NezPifs/Nez-Code'
$InstallDir = if ($env:NEZ_INSTALL_DIR) { $env:NEZ_INSTALL_DIR } else { Join-Path $env:USERPROFILE '.nez\bin' }
$Version = if ($env:NEZ_VERSION) { $env:NEZ_VERSION } else { 'latest' }
$NonInteractive = $env:NEZ_NON_INTERACTIVE -eq '1'
$Token = if ($env:NEZ_GITHUB_TOKEN) { $env:NEZ_GITHUB_TOKEN } else { $env:GITHUB_TOKEN }
$DefaultModel = 'qwen3.5:9b'

function Ask([string]$Question, [string]$Default) {
    if ($NonInteractive -or -not [Environment]::UserInteractive) { return $Default }
    $answer = Read-Host "$Question [$Default]"
    if ([string]::IsNullOrWhiteSpace($answer)) { return $Default }
    return $answer.Trim()
}

function Get-GitHub([string]$Uri, [string]$OutFile, [string]$Accept = 'application/vnd.github+json') {
    $headers = @{ Accept = $Accept; 'User-Agent' = 'nez-installer' }
    if ($Token) { $headers.Authorization = "Bearer $Token" }
    if ($OutFile) {
        Invoke-WebRequest -Uri $Uri -Headers $headers -OutFile $OutFile -UseBasicParsing | Out-Null
    } else {
        Invoke-RestMethod -Uri $Uri -Headers $headers
    }
}

function Get-GpuMemoryMiB {
    $nvidia = Get-Command nvidia-smi -ErrorAction SilentlyContinue
    if ($nvidia) {
        $values = & nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits 2>$null
        if ($values) { return [int](($values | ForEach-Object { [int]$_ } | Measure-Object -Maximum).Maximum) }
    }
    # Display adapters report their dedicated memory in the registry; WMI caps it at 4 GB.
    $key = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
    $best = 0
    Get-ChildItem $key -ErrorAction SilentlyContinue | ForEach-Object {
        $bytes = (Get-ItemProperty $_.PSPath -Name 'HardwareInformation.qwMemorySize' -ErrorAction SilentlyContinue).'HardwareInformation.qwMemorySize'
        if ($bytes -and ($bytes / 1MB) -gt $best) { $best = [int]($bytes / 1MB) }
    }
    return $best
}

function Get-RecommendedModel {
    $mib = Get-GpuMemoryMiB
    if ($mib -le 0) { return $DefaultModel }
    $gib = [math]::Floor(($mib + 512) / 1024)
    if ($gib -ge 12) { return 'deepseek-coder-v2:16b' }
    if ($gib -ge 8) { return $DefaultModel }
    return 'qwen2.5-coder:7b-instruct'
}

function Install-Nez {
    $api = if ($Version -eq 'latest') { "https://api.github.com/repos/$Repo/releases/latest" } else { "https://api.github.com/repos/$Repo/releases/tags/$Version" }
    $release = Get-GitHub $api
    $tag = $release.tag_name
    $ver = $tag.TrimStart('v')
    $assetName = "nez-code-$ver-windows-x64.zip"
    $asset = $release.assets | Where-Object { $_.name -eq $assetName }
    if (-not $asset) { throw "Release $tag has no Windows build." }
    $sums = $release.assets | Where-Object { $_.name -eq 'SHA256SUMS' }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("nez-install-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Write-Host "Downloading Nez-Code $ver for windows-x64..."
        $zip = Join-Path $tmp $assetName
        Get-GitHub $asset.url $zip 'application/octet-stream'
        if ($sums) {
            $sumsFile = Join-Path $tmp 'SHA256SUMS'
            Get-GitHub $sums.url $sumsFile 'application/octet-stream'
            $expected = (Get-Content $sumsFile | Where-Object { $_ -match "\s$([regex]::Escape($assetName))$" }) -replace '\s.*$', ''
            $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($expected -ne $actual) { throw "Checksum mismatch for $assetName." }
        }
        $unpacked = Join-Path $tmp 'unpacked'
        Expand-Archive -Path $zip -DestinationPath $unpacked
        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
        $exe = Join-Path $InstallDir 'nez.exe'
        if (Test-Path $exe) { Move-Item -Force $exe (Join-Path $InstallDir 'nez.previous.exe') }
        Move-Item -Force (Join-Path $unpacked 'nez.exe') $exe
        $hostExe = Join-Path $unpacked 'nez-code-mode-host.exe'
        if (Test-Path $hostExe) { Move-Item -Force $hostExe (Join-Path $InstallDir 'nez-code-mode-host.exe') }
        $resources = Join-Path $unpacked 'nez-resources'
        if (Test-Path $resources) {
            $target = Join-Path $InstallDir 'nez-resources'
            if (Test-Path $target) { Remove-Item -Recurse -Force $target }
            Move-Item $resources $target
        }
        Write-Host "Installed $exe"
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not ($userPath -split ';' | Where-Object { $_ -eq $InstallDir })) {
        [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ";$InstallDir"), 'User')
        $env:Path = "$env:Path;$InstallDir"
        Write-Host "Added $InstallDir to your PATH. Open a new terminal to use 'nez'."
    }
}

function Test-Ollama {
    try { Invoke-RestMethod -Uri 'http://127.0.0.1:11434/api/version' -TimeoutSec 2 | Out-Null; return $true } catch { return $false }
}

function Install-Ollama {
    if ($env:NEZ_SKIP_OLLAMA -eq '1') { return }
    if (-not (Get-Command ollama -ErrorAction SilentlyContinue)) {
        $answer = Ask 'Install Ollama to run models locally?' 'Y'
        if ($answer -match '^[Nn]') { Write-Host "Skipping Ollama. Run 'nez setup' to use the DeepSeek API instead."; return }
        if (Get-Command winget -ErrorAction SilentlyContinue) {
            winget install --id Ollama.Ollama -e --accept-source-agreements --accept-package-agreements
        } else {
            $setup = Join-Path ([IO.Path]::GetTempPath()) 'OllamaSetup.exe'
            Invoke-WebRequest -Uri 'https://ollama.com/download/OllamaSetup.exe' -OutFile $setup -UseBasicParsing
            Start-Process -FilePath $setup -ArgumentList '/SILENT' -Wait
        }
        $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')
    }
    if (-not (Test-Ollama)) {
        Start-Process -FilePath 'ollama' -ArgumentList 'serve' -WindowStyle Hidden
        for ($i = 0; $i -lt 30 -and -not (Test-Ollama); $i++) { Start-Sleep -Seconds 1 }
    }
    $model = Ask 'Model to download' (Get-RecommendedModel)
    Write-Host "Downloading $model (this can take a while)..."
    & ollama pull $model
    if ($LASTEXITCODE -ne 0) { Write-Host "Could not download $model. Run 'ollama pull $model' later."; return }

    $nezHome = if ($env:NEZ_HOME) { $env:NEZ_HOME } else { Join-Path $env:USERPROFILE '.nez' }
    $config = Join-Path $nezHome 'config.toml'
    New-Item -ItemType Directory -Force -Path $nezHome | Out-Null
    $existing = if (Test-Path $config) { Get-Content $config -Raw } else { '' }
    if ($existing -notmatch '(?m)^model_provider') {
        # WriteAllText writes UTF-8 without a byte order mark, which TOML parsers expect.
        [IO.File]::WriteAllText($config, "model_provider = `"ollama`"`nmodel = `"$model`"`n" + $existing)
        New-Item -ItemType File -Force -Path (Join-Path $nezHome '.setup-complete') | Out-Null
    }
}

if ($null -ne $env:NEZ_INSTALL_IF_LATEST) {
    # The app-server daemon's managed updater speaks a different protocol.
    Write-Error "Managed daemon installs are not supported; run 'nez update' instead."
    exit 1
}

Install-Nez
Install-Ollama
Write-Host ''
Write-Host "Done. Run 'nez' in a project folder to start."
