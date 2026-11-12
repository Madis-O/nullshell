<#
Installs NullShell for the current user. No admin rights needed.

  .\install.ps1              build, copy the exe, add a Start menu entry
  .\install.ps1 -Desktop     also put a shortcut on the desktop
  .\install.ps1 -Uninstall   remove the exe and the shortcuts (config is kept)

The app goes to %LOCALAPPDATA%\Programs\NullShell.
#>
param(
    [switch]$Desktop,
    [switch]$Uninstall,
    # Use an already built exe instead of running cargo.
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
$repo = $PSScriptRoot
$installDir = Join-Path $env:LOCALAPPDATA "Programs\NullShell"
$exe = Join-Path $installDir "nullshell.exe"
$startMenu = Join-Path ([Environment]::GetFolderPath("Programs")) "NullShell.lnk"
$desktopLink = Join-Path ([Environment]::GetFolderPath("Desktop")) "NullShell.lnk"

if ($Uninstall) {
    foreach ($path in $startMenu, $desktopLink) {
        if (Test-Path $path) { Remove-Item $path -Force; "removed $path" }
    }
    if (Test-Path $installDir) { Remove-Item $installDir -Recurse -Force; "removed $installDir" }
    "NullShell uninstalled. Your config in $env:APPDATA\NullShell was left alone."
    return
}

if (-not $NoBuild) {
    Push-Location $repo
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally {
        Pop-Location
    }
}

$built = Join-Path $repo "target\release\nullshell.exe"
if (-not (Test-Path $built)) { throw "not found: $built (build first, or drop -NoBuild)" }

New-Item -ItemType Directory -Force $installDir | Out-Null
try {
    Copy-Item $built $exe -Force
} catch {
    throw "could not replace $exe - close any running NullShell windows and try again"
}

function New-Shortcut($path) {
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($path)
    $link.TargetPath = $exe
    $link.WorkingDirectory = $env:USERPROFILE
    $link.IconLocation = "$exe,0"
    $link.Description = "NullShell terminal"
    $link.Save()
    "shortcut: $path"
}

"installed: $exe"
New-Shortcut $startMenu
if ($Desktop) { New-Shortcut $desktopLink }
"Done. Open the Start menu and type NullShell. Right click it there to pin it to the taskbar."
