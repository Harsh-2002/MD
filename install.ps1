<#
.SYNOPSIS
    Install md — a terminal markdown renderer.
.DESCRIPTION
    Downloads the latest md release from GitHub, installs the binary,
    adds it to PATH, and sets up PowerShell tab completions.
.EXAMPLE
    irm https://raw.githubusercontent.com/Harsh-2002/MD/main/install.ps1 | iex
#>

#Requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Main {
    $arch = Get-Architecture
    $version = Get-LatestVersion

    if (-not $version) {
        Write-Host "Error: failed to fetch latest version from GitHub." -ForegroundColor Red
        Write-Host "Check your internet connection and try again." -ForegroundColor Red
        exit 1
    }

    $installDir = Get-InstallDir

    Write-Host "Installing md $version ($arch)..."

    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) "md-install-$PID"
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    try {
        # Download
        $target = "$arch-pc-windows-msvc"
        $url = "https://github.com/Harsh-2002/MD/releases/download/$version/md-$target.tar.gz"
        $tarball = Join-Path $tempDir "md.tar.gz"

        Write-Host "  Downloading $version..."
        $prevProgressPref = $ProgressPreference
        try {
            $ProgressPreference = 'SilentlyContinue'
            Invoke-WebRequest -Uri $url -OutFile $tarball -UseBasicParsing
        }
        catch {
            Write-Host "Error: failed to download from:" -ForegroundColor Red
            Write-Host "  $url" -ForegroundColor Red
            Write-Host "Release binaries may still be building. Try again in a few minutes." -ForegroundColor Red
            exit 1
        }
        finally {
            $ProgressPreference = $prevProgressPref
        }

        # Extract
        tar xzf $tarball -C $tempDir 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Host "Error: failed to extract archive." -ForegroundColor Red
            exit 1
        }

        $binary = Join-Path $tempDir "md.exe"
        if (-not (Test-Path $binary)) {
            Write-Host "Error: binary not found in archive." -ForegroundColor Red
            exit 1
        }

        # Install
        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
        $dest = Join-Path $installDir "md.exe"
        Copy-Item $binary $dest -Force

        # Verify
        $verOutput = & $dest --version 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host "Error: installed binary is not executable." -ForegroundColor Red
            exit 1
        }

        Add-ToUserPath $installDir
        Setup-Completions $dest

        Write-Host ""
        Write-Host "  md $version installed to $dest"
        Write-Host "  Restart your terminal, then run 'md --help' to get started."
        Write-Host ""
    }
    finally {
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Get-Architecture {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch ($arch) {
        'AMD64' { return 'x86_64' }
        'ARM64' { return 'aarch64' }
        default {
            Write-Host "Unsupported architecture: $arch. Supported: AMD64, ARM64." -ForegroundColor Red
            exit 1
        }
    }
}

function Get-LatestVersion {
    try {
        $prevProgressPref = $ProgressPreference
        $ProgressPreference = 'SilentlyContinue'
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/Harsh-2002/MD/releases/latest" `
            -Headers @{ 'User-Agent' = 'md-cli-installer' } `
            -UseBasicParsing
        $ProgressPreference = $prevProgressPref
        return $response.tag_name
    }
    catch {
        return $null
    }
}

function Get-InstallDir {
    return Join-Path $env:LOCALAPPDATA "Programs\md"
}

function Add-ToUserPath {
    param([string]$Dir)

    $currentPath = [System.Environment]::GetEnvironmentVariable('Path', 'User')
    if ($currentPath) {
        $entries = $currentPath -split ';' | Where-Object { $_ -ne '' }
        if ($entries -contains $Dir) {
            return
        }
        $newPath = ($entries + $Dir) -join ';'
    }
    else {
        $newPath = $Dir
    }

    [System.Environment]::SetEnvironmentVariable('Path', $newPath, 'User')

    # Update current session so the user can use md immediately after sourcing
    if (-not ($env:Path -split ';' | Where-Object { $_ -eq $Dir })) {
        $env:Path = "$env:Path;$Dir"
    }

    # Broadcast WM_SETTINGCHANGE so new Explorer/terminal windows pick up the change
    try {
        if (-not ([System.Management.Automation.PSTypeName]'MD.Installer.NativeMethods').Type) {
            Add-Type -Namespace 'MD.Installer' -Name 'NativeMethods' -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(
    IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam,
    uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
        }
        $HWND_BROADCAST = [IntPtr]0xffff
        $WM_SETTINGCHANGE = 0x001A
        $result = [UIntPtr]::Zero
        [MD.Installer.NativeMethods]::SendMessageTimeout(
            $HWND_BROADCAST, $WM_SETTINGCHANGE, [UIntPtr]::Zero,
            'Environment', 2, 5000, [ref]$result
        ) | Out-Null
    }
    catch {
        # Non-fatal — user just needs to restart their terminal
    }
}

function Setup-Completions {
    param([string]$MdBin)

    $completionsDir = Join-Path $env:LOCALAPPDATA "md\completions"
    New-Item -ItemType Directory -Path $completionsDir -Force | Out-Null

    $completionFile = Join-Path $completionsDir "md.ps1"
    & $MdBin --completions powershell 2>$null | Out-File -FilePath $completionFile -Encoding utf8

    if (-not (Test-Path $completionFile) -or (Get-Item $completionFile).Length -eq 0) {
        return
    }

    # Add sourcing line to the PowerShell profile
    $profileDir = Split-Path $PROFILE -Parent
    if (-not (Test-Path $profileDir)) {
        New-Item -ItemType Directory -Path $profileDir -Force | Out-Null
    }
    if (-not (Test-Path $PROFILE)) {
        New-Item -ItemType File -Path $PROFILE -Force | Out-Null
    }

    $sourceLine = ". `"$completionFile`""
    $profileContent = Get-Content $PROFILE -Raw -ErrorAction SilentlyContinue
    if (-not $profileContent -or -not $profileContent.Contains($completionFile)) {
        Add-Content -Path $PROFILE -Value "`n# md shell completions`n$sourceLine"
    }
}

Main
