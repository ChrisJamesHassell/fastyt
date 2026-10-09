$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\fast.yt'
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'fast.yt.lnk'
Get-Process fastyt -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -like "$dir\*" } |
    ForEach-Object { $_.CloseMainWindow() | Out-Null; $_.WaitForExit(3000) | Out-Null }
Remove-Item $lnk -ErrorAction SilentlyContinue
Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
Write-Host "Removed fast.yt"
