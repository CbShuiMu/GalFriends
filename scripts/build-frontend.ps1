param([switch]$Release)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location $taskRoot
try {
    $profile = 'debug'
    if ($Release) { cargo build -p galfriends-ui --target wasm32-unknown-unknown --release --locked; $profile = 'release' }
    else { cargo build -p galfriends-ui --target wasm32-unknown-unknown --locked }
    if ($LASTEXITCODE -ne 0) { throw 'Rust 前端编译失败' }
    New-Item -ItemType Directory -Path dist -Force | Out-Null
    & .\tools\wasm-bindgen.exe --target web --out-dir dist/pkg --out-name galfriends_ui "target/wasm32-unknown-unknown/$profile/galfriends_ui.wasm"
    if ($LASTEXITCODE -ne 0) { throw 'WebAssembly 绑定生成失败' }
    Copy-Item web/index.html,web/styles.css,web/theme.css,web/characters.css,web/shell.css,web/appearance-palettes.css,web/preferences.css,web/connections.css,web/character-tags.css,web/detail-tag-reorder.js,web/press-gesture.js,web/preferences.js,web/library.css,web/collections.css,web/bridge.js,web/transitions.js,web/home.css,web/home.js,web/home-backdrop.js,web/home-render.js,web/theme-transition.js,web/toolbar.js,web/navigation.js,web/gamepad.js -Destination dist -Force
    Copy-Item web/voice.css,web/emotions.css -Destination dist -Force
    Copy-Item web/companion.html,web/companion.js,web/companion.css -Destination dist -Force
    Copy-Item web/progress-notice.js,web/choices.js,web/library-selection.js,web/library-reorder.js -Destination dist -Force
    Copy-Item web/assets -Destination dist -Recurse -Force
} finally { Pop-Location }

