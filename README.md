<div align="center">

<img src="web/assets/galfriends-logo.png" width="80" height="80" alt="GalFriends 标志" />

# GalFriends

**在 Windows 上收藏 Galgame 角色、整理立绘，和喜欢的角色对话。**

Windows x64 · 简体中文 / 日本語 / English

[快速上手](#快速上手) · [功能介绍](#功能介绍) · [数据存放位置](#数据存放位置) · [反馈问题](https://github.com/CbShuiMu/GalFriends/issues)

</div>

GalFriends 从 [GalGear](https://github.com/CbShuiMu/GalGear) 派生，沿用它的桌面界面、主题和手柄导航。你可以把不同站点的角色资料整理到一个本地角色库，为角色挑选立绘，或打开独立的桌面角色窗口聊天。

## 快速上手

当前提供源码开发预览。构建需要 x64 Rust MSVC 工具链（通过 rustup 安装）、Microsoft C++ Build Tools、WebView2 和 Python。系统组件的安装步骤见 [Tauri 的 Windows 构建依赖](https://v2.tauri.app/start/prerequisites/#windows)。

在 PowerShell 中运行：

```powershell
git clone https://github.com/CbShuiMu/GalFriends.git
cd GalFriends
.\scripts\setup.ps1
.\scripts\dev.ps1
```

首次准备会下载 WASM 构建工具、waifu2x 和本地抠图模型。模型与下载校验见 [主人物抠图说明](docs/artwork-cutout.md)。开发预览依赖工作区中的脚本与环境。

1. 在**角色库**展开添加菜单，选择**在线搜索**。选好数据源后，搜索角色并核对来源条目。
2. 导入时，应用会处理可用的 Bangumi 立绘。导入完成后，在角色详情中选择要展示的立绘。首页、角色库和收藏夹会同步使用这一选择。
3. 用**收藏夹**整理角色，或在角色详情点击麦克风打开桌面角色窗口。使用对话前，在**设置 → AI**保存服务凭据并选择模型。

## 功能介绍

| 功能 | 说明 |
| --- | --- |
| 角色资料 | 从 Bangumi、鲲 Gal（NextMoe）和 VNDB 查询、导入角色。跨来源关联由用户确认。保留别名、出处、声优、剧透标记和 API 响应快照。批评空间用于查询作品资料。 |
| 立绘处理 | Bangumi 立绘在本地进行主人物分割、透明边缘处理和 waifu2x 2× 超分。分别保存原图、透明立绘和超分结果。导入完成后可离线浏览。 |
| 角色库与收藏夹 | 支持按名字和别名搜索、按标签筛选、排序、批量整理和两级收藏夹。标签来自鲲 Gal 或 VNDB，可在本地改名、隐藏和排序。 |
| 桌面角色陪伴 | 使用当前立绘打开透明窗口。支持置顶、缩放和中日双语对话，可选 OpenAI、DeepSeek、Qwen 或 Kimi。有声播放需要绑定该角色的日语模型。 |
| 日语声音工作台 | 使用自行准备、有权使用的本地素材训练角色声音。可选 IndexTTS 2.5、GPT-SoVITS v5Turbo 或 v2Pro。可配置 GalGear 游戏库路径以只读匹配角色出演的作品。 |
| 显示与操作 | 支持粉色、白粉和白色主题及对应的深色模式，支持中日英界面、字号调节、键盘和手柄导航。 |

在线导入需要访问对应站点，可在设置中配置代理。Bangumi 和鲲 Gal 支持浏览器授权。AI 对话会向所选服务发送角色资料和近期对话，并可能产生 API 费用。语音环境、模型准备、对话数据范围与情绪参考音频见 [声音工作台](docs/voice-training.md) 和 [桌面角色陪伴](docs/desktop-companion.md)。

## 数据存放位置

| 内容 | 位置 |
| --- | --- |
| 当前角色、收藏与设置 | 可执行文件旁的 `characters/library.json` |
| 角色归档、立绘及语音绑定 | `characters/character-<身份摘要>/` |
| 账号和 API 凭据 | Windows 凭据管理器 |
| 本地抠图模型 | `%LOCALAPPDATA%/GalFriends/models/` |
| 声音训练产物 | 在声音工作台选择的输出目录 |

备份时请复制整个 `characters/`，并保留单独存放的语音模型和参考素材。换机后需要重新配置凭据。确认删除角色会清除该角色的收藏关联、历史归档和专属立绘。首次启动可复制迁入旧 GalFriends 库，原文件会保留。

## 开发与反馈

项目使用 Rust 2024 workspace、Tauri 2、Rust/WASM 和原生 JavaScript/CSS。模块职责、构建命令和检查项见 [架构说明](docs/architecture.md)、[验证记录](docs/validation.md) 和 [协作规范](AGENTS.md)。

公开仓库保留神堂刹那的示例资料与[来源清单](setsuna-album/album.json)。本地开发样图、游戏素材、录音、模型权重和用户数据随各自的本地环境保存。若示例角色没有立绘，可通过在线搜索导入带立绘的角色。

遇到问题时，可以在 [Issues](https://github.com/CbShuiMu/GalFriends/issues) 提供操作步骤、环境和脱敏后的报错。

感谢 GalGear、waifu2x 及各数据源的开发者和维护者。项目采用 [GPL-3.0-only](LICENSE)，第三方组件和本地样本的来源见 [NOTICE](NOTICE)。
