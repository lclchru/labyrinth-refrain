param([Parameter(Mandatory=$true)][string]$Media)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$env:REFRAIN_MEDIA = (Resolve-Path -LiteralPath $Media).Path
function Run-Step([scriptblock]$Action) { & $Action; if ($LASTEXITCODE -ne 0) { throw "Build step failed" } }
Push-Location $repo
try {
    Run-Step { python tools/patches.py apply --original $env:REFRAIN_MEDIA --patch patches/windows --output build/refrain-ru --replace }
    Run-Step { python tools/translate.py }
    Run-Step { python tools/subtitles.py }
    Run-Step { python tools/telop_word.py }
    Push-Location platforms/windows/runtime
    try { Run-Step { cargo build --locked --release --target i686-pc-windows-msvc } }
    finally { Pop-Location }
    Run-Step { python tools/pack_build.py --check }
    $metadata = "hud.txt", "labels.txt", "loading-hide.txt", "loading-now", "own-text.off", "rects.txt", "subs.txt"
    New-Item -ItemType Directory -Force -Path dist/refrain-ru | Out-Null
    foreach ($name in $metadata) {
        $source = Join-Path "build/refrain-ru" $name
        if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination dist/refrain-ru }
    }
} finally { Pop-Location }
