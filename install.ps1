# delta installer for Windows (PowerShell 5.1+)
# Usage:
#   iwr -useb https://raw.githubusercontent.com/tbrittain/delta/main/install.ps1 | iex
#   Or to specify a directory:
#   & ([scriptblock]::Create((iwr -useb https://raw.githubusercontent.com/tbrittain/delta/main/install.ps1).Content)) -InstallDir "C:\Tools"

param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\delta"
)

$ErrorActionPreference = "Stop"
$Repo = "tbrittain/delta"
$Binary = "delta"

# ── Architecture ──────────────────────────────────────────────────────────────

$arch = $env:PROCESSOR_ARCHITECTURE
switch ($arch) {
    "AMD64"  { $Target = "x86_64-pc-windows-msvc" }
    "x86"    { Write-Error "32-bit Windows is not supported."; exit 1 }
    "ARM64"  { Write-Error "ARM64 Windows is not yet supported. Download the x86_64 binary manually from https://github.com/$Repo/releases/latest"; exit 1 }
    default  { Write-Error "Unknown architecture: $arch"; exit 1 }
}

# ── Latest release ────────────────────────────────────────────────────────────

Write-Host "Fetching latest release..."
$ApiUrl = "https://api.github.com/repos/$Repo/releases/latest"
$Release = Invoke-RestMethod -Uri $ApiUrl -UseBasicParsing
$Version = $Release.tag_name

if (-not $Version) {
    Write-Error "Could not determine latest version. Is the repository public?"
    exit 1
}

$ArchiveName = "$Binary-$Version-$Target.zip"
$DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$ArchiveName"

Write-Host "Installing $Binary $Version for $Target"
Write-Host "From: $DownloadUrl"

# ── Download ──────────────────────────────────────────────────────────────────

$TmpDir = Join-Path $env:TEMP "delta-install-$([System.IO.Path]::GetRandomFileName())"
New-Item -ItemType Directory -Path $TmpDir | Out-Null

try {
    $ZipPath = Join-Path $TmpDir $ArchiveName
    Write-Host "Downloading..."
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipPath -UseBasicParsing

    Write-Host "Extracting..."
    Expand-Archive -Path $ZipPath -DestinationPath $TmpDir -Force

    # ── Install ───────────────────────────────────────────────────────────────

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir | Out-Null
    }

    $ExeName = "$Binary.exe"
    $Dest = Join-Path $InstallDir $ExeName
    $ExtractedExe = Join-Path $TmpDir "$Binary-$Version-$Target\$ExeName"

    if (Test-Path $Dest) {
        Write-Host "Replacing existing install at $Dest"
        Remove-Item $Dest -Force
    }

    Copy-Item $ExtractedExe $Dest
    Write-Host ""
    Write-Host "Installed: $Dest"

    # ── PATH ──────────────────────────────────────────────────────────────────

    $UserPath = [Environment]::GetEnvironmentVariable("PATH", "User") ?? ""
    $PathParts = $UserPath -split ";" | Where-Object { $_ -ne "" }

    if ($PathParts -notcontains $InstallDir) {
        $NewPath = ($PathParts + $InstallDir) -join ";"
        [Environment]::SetEnvironmentVariable("PATH", $NewPath, "User")
        Write-Host ""
        Write-Host "Added $InstallDir to your user PATH."
        Write-Host "Restart your terminal for the PATH change to take effect."
    }

    Write-Host ""
    Write-Host "Done. Run 'delta --help' to get started."

} finally {
    Remove-Item $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
