$ErrorActionPreference = 'Stop'
& "$PSScriptRoot/build-frontend.ps1"
if ($LASTEXITCODE -ne 0) { throw 'Frontend failed' }
Push-Location (Split-Path -Parent $PSScriptRoot)
try { cargo run -p galfriends --locked } finally { Pop-Location }
