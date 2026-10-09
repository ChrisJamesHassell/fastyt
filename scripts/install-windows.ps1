$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot "..")

cargo build --release
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$dir = Join-Path $env:LOCALAPPDATA 'Programs\fast.yt'
$exe = Join-Path $dir 'fastyt.exe'
New-Item -ItemType Directory -Force $dir | Out-Null

Get-Process fastyt -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $exe } |
    ForEach-Object { $_.CloseMainWindow() | Out-Null; $_.WaitForExit(3000) | Out-Null }
Copy-Item 'target\release\fastyt.exe' $exe -Force

$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'fast.yt.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($lnk)
$shortcut.TargetPath = $exe
$shortcut.WorkingDirectory = $dir
$shortcut.IconLocation = "$exe,0"
$shortcut.Description = 'fast.yt: fast, ad-free YouTube'
$shortcut.Save()

Write-Host "Installed $exe"
Write-Host "Start menu: $lnk"
