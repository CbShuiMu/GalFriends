param([string]$TorchWheel = '', [ValidateSet('v5turbo', 'v2Pro')][string]$Version = 'v5turbo')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskVoice = Join-Path $taskRoot 'tools\voice'
$taskSource = Join-Path $taskVoice 'gpt-sovits'
$taskRevision = '6d51ffceafbf240145619af4c92408a6e54b7924'
if (!(Test-Path -LiteralPath $taskSource)) {
    git clone --no-checkout https://github.com/RVC-Boss/GPT-SoVITS.git $taskSource
    if ($LASTEXITCODE) { throw 'GPT-SoVITS 下载失败' }
    git -C $taskSource checkout $taskRevision
    if ($LASTEXITCODE) { throw 'GPT-SoVITS 固定版本检出失败' }
}
if ((git -C $taskSource rev-parse HEAD) -ne $taskRevision) { throw 'GPT-SoVITS 源码版本不匹配' }
$taskPython = Join-Path $taskSource '.venv\Scripts\python.exe'
if (!(Test-Path -LiteralPath $taskPython)) {
    uv venv --python 3.11 (Join-Path $taskSource '.venv')
    if ($LASTEXITCODE) { throw '独立 Python 环境创建失败' }
}
# 可显式复用已有官方 wheel，但只读复制安装，不修改其他引擎的环境。
$taskTorch = if ($TorchWheel) { (Resolve-Path -LiteralPath $TorchWheel).Path } else { 'torch==2.8.0' }
uv pip install --python $taskPython --link-mode copy $taskTorch 'torchaudio==2.8.0' --index-url https://download.pytorch.org/whl/cu128
if ($LASTEXITCODE) { throw 'CUDA PyTorch 安装失败' }
uv pip install --python $taskPython --link-mode copy -r (Join-Path $PSScriptRoot 'gpt-sovits-requirements.txt')
if ($LASTEXITCODE) { throw 'GPT-SoVITS 训练与日语依赖安装失败' }
$taskRuntime = Join-Path $taskSource 'runtime'
New-Item -ItemType Directory -Path $taskRuntime -Force | Out-Null
& $taskPython -B -c 'import imageio_ffmpeg,shutil,sys; shutil.copyfile(imageio_ffmpeg.get_ffmpeg_exe(),sys.argv[1])' (Join-Path $taskRuntime 'ffmpeg.exe')
if ($LASTEXITCODE) { throw 'FFmpeg 准备失败' }
& $taskPython -B -c 'import torch,pyopenjtalk; assert torch.cuda.is_available(); pyopenjtalk.g2p("こんにちは")'
if ($LASTEXITCODE) { throw 'CUDA 或日语词典检查失败' }
$taskModels = Join-Path $taskSource 'GPT_SoVITS\pretrained_models'
& $taskPython -B (Join-Path $PSScriptRoot 'gpt-sovits-models.py') $taskModels --version $Version
if ($LASTEXITCODE) { throw 'GPT-SoVITS 模型下载或哈希校验失败' }
& $taskPython -X utf8 -B (Join-Path $PSScriptRoot 'gpt-sovits-language.py') $taskSource
if ($LASTEXITCODE) { throw '日语分段组件准备失败' }
$taskVariant = $Version.ToLowerInvariant()
$taskConfig = if ($taskVariant -eq 'v2pro') { 'gpt-sovits-v2pro-runtime.json' } else { 'gpt-sovits-runtime.json' }
$taskRuntimeJson = @{engine="gpt_sovits_$taskVariant";python=$taskPython;source=$taskSource;models=$taskModels;revision=$taskRevision;model_revision='8f8857563c040d542de108a2c18b0425abd6c9c5'} | ConvertTo-Json
[IO.File]::WriteAllText((Join-Path $taskVoice $taskConfig), $taskRuntimeJson)
uv pip freeze --python $taskPython | Set-Content (Join-Path $taskVoice 'gpt-sovits-installed.txt') -Encoding utf8
Write-Output "GPT-SoVITS $Version 环境与官方模型已验证。"
