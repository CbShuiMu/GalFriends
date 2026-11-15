$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$engine = Join-Path $taskRoot 'tools/waifu2x'
Get-ChildItem -LiteralPath (Join-Path $taskRoot 'setsuna-album/cutouts') -Filter '*.png' | ForEach-Object {
    $output = Join-Path $taskRoot "web/assets/setsuna/$($_.BaseName)-2x.png"
    if (-not (Test-Path -LiteralPath $output)) {
        & (Join-Path $engine 'waifu2x-ncnn-vulkan.exe') -i $_.FullName -o $output -n 1 -s 2 -t 128 -j '1:1:1' -m (Join-Path $engine 'models-cunet') 2> (Join-Path $taskRoot 'artifacts/waifu2x.log')
        if ($LASTEXITCODE -ne 0) { throw "waifu2x failed: $($_.Name)" }
    }
}
