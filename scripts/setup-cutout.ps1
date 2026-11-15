$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$python = Join-Path $taskRoot '.venv/Scripts/python.exe'
if (-not (Test-Path -LiteralPath $python)) {
    python -m venv (Join-Path $taskRoot '.venv')
    if ($LASTEXITCODE -ne 0) { throw '创建抠图 Python 环境失败，请先安装 Python。' }
}
& $python -c 'import importlib.util; raise SystemExit(0 if importlib.util.find_spec("pip") else 1)'
if ($LASTEXITCODE -ne 0) {
    & $python -m ensurepip
    if ($LASTEXITCODE -ne 0) { throw '初始化 pip 失败。' }
}
& $python -m pip install -r (Join-Path $taskRoot 'requirements.txt')
if ($LASTEXITCODE -ne 0) { throw '安装抠图依赖失败。' }
# 复用原脚本的模型路径、URL、校验和与原子下载，不另存一份模型参数。
Push-Location $taskRoot
try {
    & $python -B -c 'import hashlib; import character_art as art; model = art.default_model_path(); session = art.http_session(None); art.download_file(session, art.MODEL_URL, model, art.MODEL_SHA256) if not model.is_file() else None; assert hashlib.sha256(model.read_bytes()).hexdigest() == art.MODEL_SHA256, "ISNet Anime model checksum mismatch"; session.close(); print("ISNet Anime ready:", model)'
    if ($LASTEXITCODE -ne 0) { throw '准备抠图模型失败。' }
    & $python -B -c 'import character_foreground as foreground; foreground.prepare_models()'
    if ($LASTEXITCODE -ne 0) { throw '准备主人物分割模型失败。' }
} finally { Pop-Location }
