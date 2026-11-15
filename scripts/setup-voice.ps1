param(
    [string]$TtsRoot = '',
    [string]$GalGearRoot = '',
    [string]$Python = '',
    [switch]$SkipModels
)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskParent = Split-Path -Parent $taskRoot
if (!$TtsRoot) { $TtsRoot = Join-Path $taskParent 'TTS\index-tts' }
if (!$GalGearRoot) { $GalGearRoot = Join-Path $taskParent 'GalGear' }
$taskVoice = Join-Path $taskRoot 'tools\voice'
New-Item -ItemType Directory -Path $taskVoice -Force | Out-Null

function Get-VerifiedArchive([string]$Url, [string]$Destination, [string]$Hash) {
    if (!(Test-Path -LiteralPath $Destination)) {
        Invoke-WebRequest $Url -OutFile "$Destination.partial" -TimeoutSec 300
        if ((Get-FileHash "$Destination.partial" -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Hash) { throw "校验失败: $Destination.partial" }
        Move-Item -LiteralPath "$Destination.partial" -Destination $Destination
    }
    if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Hash) { throw "校验失败: $Destination" }
}

$taskSource = Join-Path $taskVoice 'index-tts'
if (!(Test-Path -LiteralPath $taskSource)) {
    git clone https://github.com/index-tts/index-tts.git $taskSource
    if ($LASTEXITCODE) { throw 'IndexTTS 下载失败' }
    git -C $taskSource checkout d9e41aac89fd00b3d71497fddb287b7f24613712
    if ($LASTEXITCODE) { throw 'IndexTTS 固定版本检出失败' }
}
if ((git -C $taskSource rev-parse HEAD) -ne 'd9e41aac89fd00b3d71497fddb287b7f24613712') { throw 'IndexTTS 源码版本不匹配' }
if (!$Python) {
    $Python = Join-Path $TtsRoot '.venv\Scripts\python.exe'
    if (!(Test-Path -LiteralPath $Python)) {
        uv sync --project $taskSource --frozen
        if ($LASTEXITCODE) { throw 'IndexTTS 独立环境安装失败' }
        $Python = Join-Path $taskSource '.venv\Scripts\python.exe'
    }
}
$Python = (Resolve-Path -LiteralPath $Python).Path
& $Python -B -c 'import torch, soundfile, torchaudio, transformers, omegaconf, fugashi, tiktoken; assert torch.cuda.is_available(), "CUDA unavailable"'
if ($LASTEXITCODE) { throw '语音训练依赖检查失败；已有 Python 环境不会自动修改' }

$taskPhpZip = Join-Path $taskVoice 'php.zip'
Get-VerifiedArchive 'https://downloads.php.net/~windows/releases/archives/php-8.5.12-1-nts-Win32-vs17-x64.zip' $taskPhpZip '2a36a2edcb7a5b9252f638fd5e4cf84d7f6e066092654220dc0b63e5cf4e9723'
if (!(Test-Path (Join-Path $taskVoice 'php\php.exe'))) { Expand-Archive $taskPhpZip (Join-Path $taskVoice 'php') }
$taskWs2Zip = Join-Path $taskVoice 'advhd.zip'
# 固定源码归档的 SHA-256 由首次准备后记录，模型/工具不随应用源码提交。
Get-VerifiedArchive 'https://codeload.github.com/DarthFly/advhd_ws2_tools/zip/2253327e02868ef5e84d963e8e34e2439f7e6c18' $taskWs2Zip '0a980be2c1866b46b73c7855904740b490eedb788a559452288ce1eac8a0b584'
$taskWs2 = Join-Path $taskVoice 'ws2-source\advhd_ws2_tools-2253327e02868ef5e84d963e8e34e2439f7e6c18'
if (!(Test-Path -LiteralPath $taskWs2)) { Expand-Archive $taskWs2Zip (Join-Path $taskVoice 'ws2-source') }

$taskSkill = Join-Path $GalGearRoot 'Galgear-GalgameReverse-Skill\GalgameResourceExtract'
if (!(Test-Path (Join-Path $taskVoice 'GARbro-cli\GARbro.Console.exe'))) {
    Copy-Item (Join-Path $taskSkill 'tools\GARbro-cli') (Join-Path $taskVoice 'GARbro-cli') -Recurse
}
$taskHelper = Join-Path $GalGearRoot 'crates\core\src\ai_summary\agent_archive_tools.py'
if ((Get-FileHash -LiteralPath $taskHelper -Algorithm SHA256).Hash.ToLowerInvariant() -ne '9914e55a3285c71160e5cd233ebf58fb3c30db020478bf4c87b8880884a33cdf') { throw 'GalGear 归档适配器版本已改变，请先检查兼容性' }
Copy-Item -LiteralPath $taskHelper -Destination (Join-Path $taskVoice 'agent_archive_tools.py')
Copy-Item -LiteralPath (Join-Path $GalGearRoot 'LICENSE') -Destination (Join-Path $taskVoice 'GalGear-LICENSE')
$taskModels = Join-Path $taskVoice 'checkpoints'
New-Item -ItemType Directory -Path $taskModels -Force | Out-Null
# 复用已经安装的辅助权重时复制文件，禁止创建到其他应用数据的目录联接。
$taskCache = Join-Path $TtsRoot 'checkpoints\hf_cache'
if (!(Test-Path (Join-Path $taskModels 'hf_cache\w2v-bert-2.0\model.safetensors')) -and (Test-Path $taskCache)) {
    Copy-Item $taskCache (Join-Path $taskModels 'hf_cache') -Recurse
}
if (!$SkipModels) {
    & $Python -B (Join-Path $PSScriptRoot 'voice-models.py') $taskModels
    if ($LASTEXITCODE) { throw 'IndexTTS 模型下载或校验失败' }
}
@{python=$Python;source=$taskSource;models=$taskModels;php=(Join-Path $taskVoice 'php\php.exe');ws2=$taskWs2;garbro=(Join-Path $taskVoice 'GARbro-cli\GARbro.Console.exe');archive_tools=(Join-Path $taskVoice 'agent_archive_tools.py')} |
    ConvertTo-Json | Set-Content (Join-Path $taskVoice 'runtime.json') -Encoding utf8
Write-Output '语音运行环境已准备。模型状态由工作台再次检查；未下载辅助模型时不可训练。'

