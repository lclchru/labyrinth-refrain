param(
    [Parameter(Mandatory=$true)][string]$Game,
    [string]$Package = (Join-Path $PSScriptRoot "../../dist")
)
$ErrorActionPreference = "Stop"
$gameRoot = (Resolve-Path -LiteralPath $Game).Path
$packageRoot = (Resolve-Path -LiteralPath $Package).Path
$sourceDll = Join-Path $packageRoot "dinput8.dll"
$gameExe = Join-Path $gameRoot "refrain.exe"
if (!(Test-Path -LiteralPath $gameExe) -or !(Test-Path -LiteralPath $sourceDll)) {
    throw "Game executable or built dinput8.dll is missing"
}
$running = Get-Process -Name refrain -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $gameExe }
if ($running) { throw "Close the game before installing the patch" }
$backup = Join-Path $gameRoot ("refrain-ru-backups/" + (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Path $backup | Out-Null
$targetDll = Join-Path $gameRoot "dinput8.dll"
if (Test-Path -LiteralPath $targetDll) { Copy-Item -LiteralPath $targetDll -Destination $backup }
$metadata = Join-Path $packageRoot "refrain-ru"
if (Test-Path -LiteralPath $metadata) {
    $targetMetadata = Join-Path $gameRoot "refrain-ru"
    if (Test-Path -LiteralPath $targetMetadata) {
        Copy-Item -LiteralPath $targetMetadata -Destination $backup -Recurse
    }
    New-Item -ItemType Directory -Force -Path $targetMetadata | Out-Null
    Get-ChildItem -LiteralPath $metadata -File | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $targetMetadata -Force
    }
}
Copy-Item -LiteralPath $sourceDll -Destination $targetDll -Force
Write-Output "Installed. Backup: $backup"
