# 日语角色声音训练

左侧 `mic` 导航进入独立声音工作台。选择 IndexTTS 2.5、GPT-SoVITS v5Turbo 或 GPT-SoVITS v2Pro，再在「游戏关联」选择角色及关联游戏。程序从手动关联或 GalGear 安装条目读取游戏路径。点击 upload 图标的「音频提取」启动 AI 提取。映射 JSON 下方的 linked_services 按钮「解包并识别说话人」优先读取已填入的映射，映射为空时也会调用 AI 提取。

完成后自动填入映射路径和「素材中的说话人 · 语音数」。仅有一个说话人时自动选中，多人结果需要手动选择。重新读取同一映射时保留仍有效的选择。核对结果后可开始训练。当前只开放日语，后台拒绝中文或语言不一致的映射。切换引擎重新检查对应环境，各引擎模型独立保存。

顶部的下载按钮提供 IndexTTS 2.5、GPT-SoVITS v5Turbo、GPT-SoVITS v2Pro 与 Irodori-TTS v4.1 Small 的基础权重。安装到 `tools/voice/`，先校验已有文件再下载缺失或损坏的文件，下载完成后核对大小和 SHA-256。取消按钮在顶部，已完成校验的文件可在重试时复用。训练所需的源码和 Python 环境仍由下文的准备脚本配置。Irodori 目前只提供权重下载。

语音素材、训练任务和模型自动保存到可执行文件旁的 `characters/<罗马字>--<身份摘要>/voice/`。已完成模型会直接出现在悬浮角色的模型列表中。「游戏关联」右下角的文件夹加号可通过系统目录选择器登记旧训练结果，支持选择模型本身或包含模型的上一级目录。

AI 生成的对白与语音映射保存在该角色目录的 `voice/skill-extract-<任务ID>/dialogue-voice.json`，与本次音频、脚本副本及提取报告一起保留。验证文件成功后，界面回填这个本地 JSON 的路径。

「训练日志」显示当前阶段、可用的步数，以及训练器实时输出。IndexTTS 每轮输出训练和验证损失，GPT-SoVITS 同时保留各阶段的完整磁盘日志。界面保留最近一段输出，向上滚动可查看已显示的日志；导航离开后回到工作台可继续查看当前训练。

## GPT-SoVITS v5Turbo

运行 `scripts/setup-gpt-sovits.ps1`；可用 `-TorchWheel` 指定已有官方 PyTorch 2.8.0 CUDA 12.8 wheel，只读复制到新的 `.venv`。训练器需要自己的 PyTorch、Lightning、PEFT 和日语音素依赖，不修改 IndexTTS 环境。安装训练/推理所需的官方依赖子集，Gradio 是上游音频工具的导入依赖，但不启动 WebUI、ASR 或人声分离服务。

- 官方源码：[RVC-Boss/GPT-SoVITS](https://github.com/RVC-Boss/GPT-SoVITS/tree/6d51ffceafbf240145619af4c92408a6e54b7924)，固定提交 `6d51ffceafbf240145619af4c92408a6e54b7924`，MIT 许可证。
- 官方模型：[lj1995/GPT-SoVITS](https://huggingface.co/lj1995/GPT-SoVITS/tree/8f8857563c040d542de108a2c18b0425abd6c9c5)，固定 revision `8f8857563c040d542de108a2c18b0425abd6c9c5`。使用 `s1v3.ckpt`、`gsv-v5-pretrained/s2Gv5turbo.pth` 和同目录 `vocoder.pth`；按官方组合使用 HuBERT、RoBERTa 辅助权重。文件名中的 `chinese` 是官方组件名称；训练条目和推理语言全部为日语。
- 安装信息独立写入 `tools/voice/gpt-sovits-runtime.json`，可用 `GALFRIENDS_GPT_SOVITS_RUNTIME` 覆盖。下载支持断点分段重试，最终校验官方文件大小和 LFS SHA-256。安装包和模型不提交或分发。
- 安装时预先准备日语分段所需的 fastText `lid.176.bin`，核对上游组件使用的 MD5 并记录 SHA-256；推理所需的 `resampy` 也在固定依赖中，避免首次生成时缺少重采样组件。

每次训练创建 `gpt-sovits-v5turbo-随机ID/`。`audio/` 中每个 32 kHz 单声道 `.wav` 都有同名 UTF-8 `.txt`，`dataset.list` 按官方 `音频绝对路径|说话人|ja|对应台词` 格式保存；台词来自 WS2 指令映射，去除显示控制串，不用翻译或 ASR 猜测。`dataset.json` 保留原文、原音频哈希、脚本引用和排除原因。

依次调用官方音素、HuBERT、语义特征脚本，以及 `s1_train.py`、`s2_train_v3_lora.py`；每个特征阶段核对完整样本集合，任何漏项或重复项都失败。默认两阶段各 3 轮，GPT batch 4，SoVITS batch 2、LoRA rank 16 和梯度检查点，适配 8 GB GPU。GPT 的官方语速/音素长度筛选可能进一步减少数据，`gpt-dataset.json` 记录实际保留与排除的文件，`model.json` 分别记录 GPT 和 SoVITS 的样本数。官方训练器默认不执行独立验证集评估，完成训练不等于已经证明泛化或主观音色质量。

Windows 单卡通过轻量入口调用官方 GPT `main`，将 Lightning 策略设为单设备并显式指定 sampler 的 `num_replicas=1, rank=0`，避免上游对单卡也建立 Gloo 通信。模型、损失、优化器和训练循环保持官方实现。上游 i18n 不支持跨盘相对路径，因此临时工作目录放在源码同盘的 `tools/voice/gpt-sovits-work/`，最终数据和模型位于角色的 `voice/` 目录。

完整产物为 `gpt.ckpt`、`sovits.pth`、`reference.wav`、对应 `reference.txt` 与 `model.json`。元数据固定格式 `galfriends-gpt-sovits-v5turbo-v1`，记录角色 ID、日语、来源提交、权重哈希及数据量。每阶段日志与 `job.json` 保留在任务目录，取消清理进程树，不覆盖旧模型。桌面角色窗口可分别绑定各引擎，见 [桌面角色陪伴](desktop-companion.md)。

## GPT-SoVITS v2Pro

运行 `scripts/setup-gpt-sovits.ps1 -Version v2Pro`。与 v5Turbo 共用官方源码、Python 环境和通用权重，并额外校验 `v2Pro/s2Gv2Pro.pth`、`v2Pro/s2Dv2Pro.pth` 和 `sv/pretrained_eres2netv2w24s4ep4.ckpt`。独立运行配置写入 `tools/voice/gpt-sovits-v2pro-runtime.json`，可用 `GALFRIENDS_GPT_SOVITS_V2PRO_RUNTIME` 覆盖。

每个 `gpt-sovits-v2pro-随机ID/` 保留同名 WAV/TXT、`dataset.list` 与台词来源。HuBERT 阶段之后通过官方 `2-get-sv.py` 为每段语音生成说话人特征，然后用 v2Pro 底模提取语义特征。GPT 与 SoVITS 默认各训练 3 轮。SoVITS 使用官方 `s2_train.py` 和 v2Pro 生成器、判别器全量微调，单卡批次为 1。输出 `sovits.pth` 是可直接用于官方推理器的生成器权重，模型元数据格式为 `galfriends-gpt-sovits-v2pro-v1`。

## 运行环境

在根目录执行 `scripts/setup-voice.ps1`。源码与模型保存在忽略的 `tools/voice/`，不加入普通发行包。优先只读复用相邻 `TTS/index-tts/.venv` 的 Python；也可传 `-Python`。缺少已安装解释器时使用 uv 创建本应用的独立环境。依赖不足明确失败，不自动改写其他应用的环境。

固定来源：

- [IndexTTS 官方源码](https://github.com/index-tts/index-tts/tree/d9e41aac89fd00b3d71497fddb287b7f24613712)，提交 `d9e41aac89fd00b3d71497fddb287b7f24613712`。
- [IndexTeam/IndexTTS-2.5](https://huggingface.co/IndexTeam/IndexTTS-2.5/tree/c39ce5ba981572cb187443877ff559dfb246ce63)，revision `c39ce5ba981572cb187443877ff559dfb246ce63`。`voice-models.py` 校验所有下载文件大小及 LFS SHA-256，写 `verified-models.json`。省略训练不使用的 Qwen 文本情绪模型。
- GalGear 的 `agent_archive_tools.py`：只复用安全成员路径检查和 GARbro 列表解析，保留 GPL 许可证；不运行旧剧情宿主，不读取其配置或凭据。
- GalExtract 随附 `GARbro-Mod-1.0.2.2`，复制工具完整运行依赖、来源清单和许可证。
- [AdvHD WS2 读取器](https://github.com/DarthFly/advhd_ws2_tools/tree/2253327e02868ef5e84d963e8e34e2439f7e6c18)，固定提交 `2253327e02868ef5e84d963e8e34e2439f7e6c18`；PHP 8.5.12 NTS x64 的官方 ZIP 校验 SHA-256，保留上游许可。

工具安装信息写在 `tools/voice/runtime.json`，支持 `GALFRIENDS_VOICE_RUNTIME` 覆盖位置。当前是开发预览，未配置携带 Python/CUDA/模型的独立安装包。

准备脚本同时下载并校验固定版本的 Wav2Vec2-BERT、CAMPPlus 和 BigVGAN 辅助权重；已有本地副本也必须通过大小及哈希校验。基础模型是官方多语种模型，角色适配器的语料、token 前缀、语言嵌入和输出元数据统一使用 `ja`。辅助说话人编码器的文件名不代表角色训练语言。

## 解包与映射

当前自动适配 **AdvHD 1.9 的单字节 WS2 指令布局**，支持的实际样本为 Nightmare×Sisters。它不是所有 Galgame 的通用解包承诺。固定读取器逐条读取完整指令，未知操作码导致该脚本失败，不通过搜索字节猜台词。读取器空编码误走 UTF-16 的问题由调用层 `SingleByteReader` 修正，第三方源码保持不变。

每次任务在角色的 `voice/` 下创建全新的 `extract-随机ID/`，按归档保留 `raw/Voice`、`raw/Rio`、`raw/Rio1`、`raw/Script`。提取前检查所有成员名，拒绝路径穿越、重复成员、符号链接及 Windows 重解析点；核对 GARbro stderr、实际文件清单，不以其退出码独自判成功。原游戏与存档不写入，中文补丁目录不读取。

`dialogue-voice.json` 保存 `schema: 1`、`game_dir`、`asset_root`、`original_language: ja`、`entries` 及失败脚本清单。每条记录包含原日文台词、说话人、日语语音路径、原生消息 ID、脚本 SHA-256 和消息/姓名/语音指令偏移。语音来自实际 `0x28` 指令，姓名来自 `0x15`，台词来自 `0x14`；每次消息消耗后清除语音引用。双人共同署名单独列出，由用户明确选择，不混入单角色语料。

工作台可读取上述映射。其他引擎应先用 GalExtract 生成并核验实际原文映射，再显式转换为此协议；GalTTS 的 `chinese_text` 替换语音映射不能直接作为日语训练文本。`extraction-report.json` 区分完整解包和脚本解析失败。

## 训练与产物

`core/voice` 定义共享协议，`src-tauri/voice` 只检查角色身份、管理任务互斥和 Tauri Channel，`core/voice/process` 管理子进程和取消。Python 工作模块处理素材、模型和训练。网络/模型计算不持有角色库锁，导航与收藏可继续使用。输入 JSON 通过 stdin 传递，不拼接 shell 命令。Windows Job 管理任务后代，父进程退出和取消会终止工作进程，已完成文件保留。

基础模型、游戏目录、输出目录不能重叠。规范化音频、特征和训练权重全部写入新的 `training-随机ID/`。音频按内容哈希去重；相同音频对应不同台词时报错。仅使用匹配说话人的日语条目；按固定读取器规则清理 `%LF`、`%K%P` 和换行，在训练副本中保留原文追溯。不接受无正文、无有效信号、少于 1 秒或超过 12 秒的样本，排除原因写入 `dataset.json`。

训练采用官方 2.5 的 multilingual tiktoken、日语 G2P、`<|ja|>` 前缀和语言嵌入、Wav2Vec2-BERT 第 17 层特征、2.5 codec 与 CAMPPlus。日语文本处理与官方推理一致，`g2p_ratio=0`。特征编码与 GPT 分阶段使用 CUDA，特征写盘后释放编码器显存；GPT 使用 BF16、批次 1、梯度检查点。LoRA rank 8，只更新注意力投影。使用真实参考情感特征，语义 token 的起始符直接对齐首个目标，不重复偏移损失。默认 3 轮、学习率 `1e-4`；8GB 显存的短样本训练步已实测，完整语料耗时随机器而异。

随机种子 42，10% 独立音频留作验证（至少一条），每轮记录实际训练/验证损失。最优适配器写入临时文件、验证可重载后原子替换任务内的 `adapter.pth`；`model.json` 记录角色 ID、说话人、`ja`、基础 GPT SHA-256、原素材映射、训练步数和验证损失，`reference.wav` 是独立锚点。基础 `gpt.pth` 不覆盖。`training.apply_adapter(model, path, torch, base_sha256)` 在核对基础模型哈希、全部模块和有限张量后将适配器加入内存模型，已接入桌面角色窗口的日语有声对话。

验证任务使用 `verification-*` 目录并写 `verification_only: true`，少量样本验证不能作为完整角色模型质量证明。完整训练、主观试听和长文本稳定性应分别验收。中断任务没有 `completed` 标记，不自动重启或把半成品宣告为成功。

## 验证命令

```powershell
python -X utf8 scripts/test_voice.py
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p galfriends-ui --target wasm32-unknown-unknown --locked -- -D warnings
.\scripts\build-frontend.ps1
cargo build -p galfriends --locked
```

所有普通测试使用临时夹具；真实解包处理用户选择的关联游戏，训练写入对应角色目录。原始语音和模型产物不随源码或发行包分发。

## 使用 GalExtract 的 AI 解包

先在设置 → 路径与工具保存 GalGear 的 `catalog.json` 或数据目录。安装版通常位于 `%APPDATA%/com.galgear.desktop`，便携版位于程序旁 `data/`。详情页相册下显示鲲 Gal 出演作品，并通过作品来源 ID 或鲲 Gal 封面内容摘要自动关联安装。训练页选择角色和关联游戏后，点击「音频提取」。

此入口复用 GalGear AI 宿主，使用 GalFriends 的 AI 设置和独立凭据。目前上游工具宿主支持 OpenAI、DeepSeek；需要已保存密钥和模型。凭据仅经标准输入传给 worker，不读取 GalGear 用户配置。无需安装或登录 Codex CLI。

GalExtract 来自 [Galgear-GalgameReverse-Skill](https://github.com/CbShuiMu/Galgear-GalgameReverse-Skill)。可在「设置 → 更新 → Skill」安装独立副本，应用优先使用 EXE 旁的 `GalFriends-Skills/`。未安装独立副本时，仍可用 `GALFRIENDS_GALGEAR_SKILL` 指定含 `SKILL.md` 的 `GalgameResourceExtract` 目录，或从关联的便携版 GalGear 及开发目录查找。用 `GALFRIENDS_GALGEAR_HOST` 指定 GalGear 的 `crates/core/src/ai_summary` 目录（需保留同目录依赖模块）。当前仍依赖源码形式的宿主。

每次创建 `skill-extract-*`，保存任务说明、工具调用审计、失败诊断、资源、`extraction-report.json` 与 `dialogue-voice.json`。AI 读取技能和真实文件后选择宿主工具；AdvHD 1.9 可选择已验证的 WS2 适配器，其他引擎可调用固定归档工具与无文件/网络访问的纯解析器。不能执行任意 shell 或游戏程序。通用解析接口要求原文、姓名、语音引用的字节证据，暂不能表达的编码或跨文件关系须保留为未解决项，不能按文件顺序猜配。

最终映射沿用日语训练协议，资源与脚本副本必须位于本次输出目录。验证通过才发布结果；解包后核对实际说话人并继续原有训练。AI 服务会接收所读取的文件信息和必要脚本片段，音频与完整产物保存在本机。所有未知引擎不保证成功，字节一致也不能替代引擎语义核验。

2026-11-15 本地连通验证：GalFriends 配置的 DeepSeek 经 GalGear 宿主读取 GalExtract，选择 AdvHD 适配器，处理 105 个脚本并生成 1,488 条关联。其中「刹那」1,113 条独立语音经训练素材筛选保留 1,109 条；`title.ws2` 解析失败保留在未解决列表。源游戏 EXE 与四个输入归档的 SHA-256 前后一致。另验证宿主 GARbro Mod 的指定成员列表与解包。按用户要求没有执行模型训练，也未将测试结果绑定为角色模型；其他引擎尚未做真实样本验证。
