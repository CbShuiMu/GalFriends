# GalFriends 架构与范围

GalFriends 从 GalGear 选择性移植 UI 基础组件，保持 Rust 2024 workspace、Rust/WASM UI、原生 JavaScript/CSS 和 Tauri 2。应用 ID 为 `com.galfriends.desktop`，库写入可执行文件旁的 `characters/library.json`，由用户配置路径后，只读访问 GalGear 游戏库和所选游戏素材。角色资料、关联与训练产物保持独立保存。

## 模块

- `crates/core/characters`：角色、来源身份、分类别名、立绘、标签和原始响应快照。角色身份独立于游戏路径，出演作品通过来源 ID 或用户确认与安装条目关联。
- `crates/core/library_ops`：批量收藏、收藏夹成员和删除的内存规则，先验证全部 ID 再变更；收藏通过 `store::set_favorites` 原子更新索引并返回身份与收藏状态，文件清理由 store 提交，命令成功后才发布结果。
- `crates/core/library_order`：共用排序类型、角色顺序补齐与整组移动；`store/order` 只原子更新索引中的 `settings.library_order`，通过 `src-tauri/library_order` 的后台短事务返回轻量顺序。`ui/library/order` 校验可见拖拽身份并编排请求，`web/library-reorder.js` 复用 GalGear 的长按、堆叠、让位、滚动与落下动画。新增和删除角色时同步补齐已有自定顺序，角色数组继续保留添加顺序。
- `crates/core/collections`：两级收藏夹的创建、改名、移动与删除。新增 `collection_parents` 映射，缺省为空以兼容旧平铺数据；改名同步子项引用，删除保留角色。命令同样使用原子保存与短事务。
- `crates/core/sources`：Bangumi、鲲 Gal（NextMoe）、VNDB 的只读搜索及详情。批评空间复用 GalGear 的 SQL 表单查询规则，仅检索作品。网络具有超时、响应体上限、错误状态检查和分页边界。
- `crates/core/archive`：按角色身份和内容摘要追加完整 JSON 版本；删除时验证全部目标边界并清除角色专属目录，拒绝符号链接与重解析点。迁移与失败语义见 [角色归档 ADR](adr/0002-character-archive.md)。
- `crates/core/tags`：来源标签的显示投影、本地改名、隐藏与排序；详情和筛选共用，API 原始标签不修改。
- `crates/core/store`：临时文件、刷盘、原子替换；`store/deletion` 持久化角色删除意图，清除归档后提交新库。中断保留意图，重试或加载时继续；未完成时阻止其他写入，避免旧状态重建已删资料。仅无删除意图且库文件不存在时提供演示样本；解析失败不重置库。
- `crates/core/artwork`：限制来源的图片下载、归档原图读取、`artwork/cutout` 对 `character_foreground.py` 的本地调用以及 waifu2x 超分。主人物检测与 SAM 2 实例选择去掉海报大头像和脚边文字，再复用原 `character_art.py` 的 ISNet Anime 边缘、alpha 38–225 和 `keep.json`。每个子进程有 120 秒上限，输出验证尺寸、透明通道和前景；失败不返回半成品。原始文件单独保留，普通再次超分直接复用透明图，明确的“重新抠图”操作才读取原图。
- `crates/ui`：导航、查询草稿、请求 ID、响应处理与页面。`library/state` 管理标签、多选与弹层草稿，`query` 处理别名搜索及当前标签源的安全标签筛选；`actions` / `toolbar` 管理角色库工具栏，`collection_actions` / `collections` 管理收藏夹独立工具栏、导航和编辑；`view` 装配页面。批量操作失败保留选择，成功才清空选择；业务状态不进入 JavaScript。
- `crates/i18n`：沿用 GalGear 的语言与消息目录机制，中文、日文、英文。
- `src-tauri`：轻量命令与独立角色目录。`artwork` 定位 Python、脚本、修正参数、模型并编排新角色全部立绘的下载 → 去背景 → 超分。网络、图片处理、保存任务运行在 `spawn_blocking`；全部处理完成才进入保存和内存发布的短事务，不持锁等待网络或子进程。重复导入已有身份不重新处理；关联来源时保留目标已有立绘。
- `web`：GalGear 的主题色、72px 图标侧栏、焦点与手柄导航。`shell.css` 统一胶囊按钮；随后加载 `appearance-palettes.css` 和 `preferences.css`，管理可选配色与设置布局；`library.css` 管理角色工具栏和左下姓名浮签及右下桌面陪伴、收藏、删除的局部悬停渐变，`collections.css` 管理独立收藏夹布局，角色详情样式在 `characters.css`。`toolbar.js` 仅负责添加菜单、排序弹层定位与焦点；`theme-transition.js` 管理主题保存后的扩散动画与迟到快照保护；`transitions.js` 管理页面、弹窗和首页进入详情的临时动画层。`home.css` / `home.js` / `home-backdrop.js` 移植 GalGear 首页的选中卡片伸缩、对齐导航与串行解码滑动淡入；`home-render.js` 保留首页动画宿主，防止 Rust 重绘打断连续切换。迟到图片解码不能更新已切走页面，系统减少动态效果偏好禁用动画。具体对照见 [UI 设计约定](ui-design.md)。

离线表情工具通过根目录 `character_expressions.py` 调用 `core/artwork/python/sprite_expressions`，读取当前所选立绘和 GARbro 导出的 PNA，使用局部特征匹配跨服装姿势，导出原生表情、偏移与验证清单。`catalog` 在角色归档目录内保存素材包，摘要验证和原子索引由 Python 公用模块统一实现。`core/artwork/expressions` 调用同一离线合成入口并验证输出，`src-tauri/expressions` 管理原始表情选择会话和提交事务，`ui/expressions` 管理 face 弹窗。打开 face 只由 Rust 读取归档时生成的原始表情近景，不调用 Python 或匹配目标。用户确认后才通过同一离线入口合成所选一项，提交前后校验当前立绘，成功后广播桌面角色图片更新；不持有库锁等待合成。详细格式、依赖和本地样本见 [离线立绘表情提取](artwork-expressions.md)。

## 导入进度

导入进度使用 `core/characters::ImportProgress` 共享协议，通过 `import_character` 可选的 `onProgress` Tauri Channel 传输。`core/artwork::prepare_with_progress` 在真实去背景与超分步骤前通知，桌面编排补充下载序号和保存阶段。UI 按当前请求 ID 接收，局部更新提示，不重绘完整页面；`web/progress-notice.js` 负责通知的入场、退场、文字动画与 DOM 保留，以及在短消息到期时请求 Rust 清理对应状态。业务进度与消息有效性仍由 Rust 决定。无 Channel 的旧调用保持兼容；窗口关闭造成通知丢失不取消既有原子导入，成功仍以保存完成的命令响应为准。

## 设置边界

声音训练与情绪参考的选择控件沿用 Rust 输出的选项、当前值和禁用状态。`web/choices.js` 仅将原生 select 增强为与角色库排序一致的下拉浮层，选择通过原有 change → `input_changed` 桥接返回 Rust；不维护第二份业务选择。排序与工作台共用弹层定位、方向键导航和主题样式。

`core/preferences` 校验离散设置值并定义只读环境信息协议；`ui/preferences` 管理分类、选项与请求编排；`src-tauri/preferences` 探测本应用路径与 waifu2x，工具定位与实际超分命令共用。`web/preferences.js` 只处理滑块几何、分区动画和键盘焦点。新增设置 `color_theme`、`font_size`、`search_source`、`ai_provider` 在旧文件中分别缺省为粉色、16px、Bangumi、OpenAI；原有明暗模式、语言、代理和标签源保留。仍通过原子保存事务确认成功，不提前发布新设置。

主题与配色通过 `src-tauri/preferences::set_appearance` 调用 `core/store::set_appearance`，复用库事务锁、删除意图保护和原子替换，只更新轻量索引；旧内嵌库首次写入仍准备完整归档。响应使用 `core/preferences::Appearance`，只含 `theme` / `color_theme`。`ui/preferences/theme` 负责请求和局部控件片段，经 `GalFriendsBridge.render_theme` 原位应用，不重新传输图片或重建页面。保存期间若其他操作重绘整页，响应恢复完整渲染；失败不发布新主题。旧 `mutate_library` 主题调用保留整库返回协议，持久化统一调用同一入口。

waifu2x 子进程以工具目录为工作目录，模型参数固定为相对路径 `-m models-cunet`。该工具会误处理 Windows `\\?\` 前缀的绝对模型路径，重复拼接工具目录并崩溃；不将 Tauri 资源目录拼出的绝对路径传给 `-m`。`verify_import_artwork` 将工具目录规范化以覆盖此差异，并允许通过第一个命令行参数指定实际桌面工具目录。

## 版本检查

更新地址是 GalFriends 自己的设置，旧库缺省使用本项目的 GitHub Releases 页面，保存继续走库的原子事务。应用在后台读取 GitHub 发布列表，按语义版本比较并包含预览版。Windows x64 的应用更新只接受已检查发行中的 `GalFriends.exe`，要求资产同时提供长度和 SHA-256。主进程下载到同卷暂存区并验证 PE 格式和摘要。独立交接进程等待主进程退出，在重新校验后备份并原子替换现有 EXE，再启动新版。替换或重启失败保留或回滚原 EXE，其他文件不参与替换。

Skill 更新下载上游 main 对应的固定 commit ZIP，在私有 `GalFriends-Skills/` 内校验边界、大小和必要资源，然后切换安装目录并保留上一版。解包与更新通过文件锁协调，不改外部 Skill 或 GalGear 的文件。下载及校验在后台执行，进度通过现有 Channel 机制传到界面，进入文件提交前可取消。

## 运行诊断

`src-tauri/diagnostics` 移植 GalGear 的每次启动独立日志，在可执行文件旁的 `logs/galfriends_启动时间.log` 记录启动、退出、命令、后台结果、前端异常及原生崩溃线索。统一边界脱敏，core 不依赖日志实现。Tauri 插件向主窗口与桌面角色窗口注入诊断传输；两个窗口均注册 WebView2 原生进程监听。日志不会自动上传，失败不改变业务结果，详见 [运行日志](runtime-logs.md)。

## 服务连接与凭据

`core/connections` 共用服务白名单、状态与模型协议；其 `client` 负责固定 API 端点的模型读取、令牌校验、有界响应和代理，`credentials` 使用 Windows Credential Manager 保存完整凭据记录，`oauth` 移植 GalGear 的 PKCE、环回回调、随机 state、取消、超时和令牌轮换逻辑。服务返回正文与已保存 Token 不发送到 UI。AI 密钥与所选模型在同一个凭据记录里原子写入，失败保留旧记录。

`src-tauri/connections` 管理每个服务的后台任务，网络与浏览器等待不持有角色库事务锁。`ui/preferences/connections` 独立跟踪请求 ID 和每张卡片的草稿/错误，因此慢速登录不阻塞本地收藏与导航。模型只从接口返回列表选择，保存前再次校验可用性；此配置页不提供生成或对话入口；角色训练位于独立的麦克风工作台。

用户明确同意复用 `galgear.cbshuimu.net` 的现有 Bangumi/NextMoe 授权回调；公开应用 ID 和服务协议仍使用原值，系统凭据目标则独立为 `GalFriends/{service}/{原应用数据目录配置路径摘要}`，迁移角色库不改变该身份。不读取原应用配置或凭据。普通测试通过本地 HTTP 夹具验证授权完整交换和取消，不自动操作真实账号登录。

角色查询通过 `CatalogClient` 仅向所选来源的固定 API 主机发送认证头，Bangumi 相册和图片请求不携带 Token；认证请求禁用重定向。API Key 与 OAuth 刷新令牌不进入 library.json。更多取舍见 [连接配置 ADR](adr/0001-service-credentials.md)。

## 数据约束

资料身份是 `source + id`，用户明确选择“关联此角色”后才合并。不能仅凭同名或罗马字确定身份。每个来源保留完整响应快照；展示层取用姓名、介绍、分类别名和标签，不丢弃未展示字段。重复关联同一来源 ID 替换该来源快照；已有收藏和立绘不丢失。

Bangumi 详情还保存作品、声优和完整相册响应。概览页的主立绘来自角色详情的 `images.large`，独立于 `/p1/characters/{id}/photos`。新导入把有效的主立绘放在相册图之前；如果同一图片已在相册中，则保留相册条目及其剧透标记。相册是网站私有接口，变化或失败时报告错误，不伪装成无图。界面不显示标记为剧透的图片，但保留来源记录。相册中“待删除”的条目不作为可选立绘。

立绘只接受 Bangumi 的 `lain.bgm.tv`。鲲 Gal 和 VNDB 返回的图片仍留在原始快照中，但不显示为立绘。标签只来自鲲 Gal 和 VNDB，保留分组及剧透标记；默认视图隐藏剧透与成人标签，原始数据不删减。

导入完成后的 `Artwork.original` 保留 Bangumi URL，新增可选 `original_data` 以 data URL 保存下载文件的原始字节；`url` 保存透明 PNG，`enhanced` 保存独立的 2× PNG。三者进入完整历史快照，当前库以轻量索引引用版本；旧记录缺少 `original_data` 时默认为空，无需重写旧库。图片 data URL 保存在完整角色版本中，选择立绘只写轻量索引。更多写入与旧库兼容规则见 [角色库轻量索引 ADR](adr/0003-library-index.md)。原脚本、原样本及修正参数均不改写。

开发预览使用项目 `.venv/Scripts/python.exe`、`character_foreground.py`、原 `character_art.py` 和 `setsuna-album/keep.json`，模型仍在 `%LOCALAPPDATA%/GalFriends/models/`。`scripts/setup-cutout.ps1` 复用原脚本的原子下载，并按 `character_foreground.models.json` 的固定 revision / SHA-256 准备模型；导入过程中不安装依赖或下载模型，缺失时显示错误。发行安装包尚未配置，不能将此预览视为已打包 Python 运行时的独立发行版。

详情页的重新处理按钮调用 `reprocess_artworks`，优先从 `original_data` 读取完整原始文件，旧数据缺原图时才重新下载。若已保存的 Bangumi 快照有尚未入库的主立绘，也下载并处理这张图。全部处理成功后，在原事务内核对图片和补图来源没有并发变化，再替换三个图片字段；补图追加在末尾，保留当时的收藏、选中立绘、标签和资料，失败保留旧结果。原图损坏不静默退回已有抠图。算法与模型说明见 [主人物抠图](artwork-cutout.md)。

## GalGear 取舍

保留：首页、角色库导航（Person 图标）、命名收藏夹、主题切换、设置、关于、原 200px / 3:4 卡片、焦点/手柄基础能力、国际化、原子保存思路和 waifu2x。角色库工具栏提供全别名搜索、标签筛选、添加顺序/姓名/来源/自定排序及适用的升降序、刷新本地库、多选收藏/取消收藏/加入收藏夹/移除。新增角色上半身裁切和首页全身立绘；首页、角色库、收藏夹卡片及详情共用当前选中立绘，优先使用该图的超分结果；详情换图即时预览并后台保存选择，失败恢复已保存状态。侧栏、关于页和网页图标共用 `web/assets/galfriends-logo.png`，图案为红眼 Q 版角色与右上角的 GalGear 樱花。Windows 使用同一图案的 `src-tauri/icons/icon.png` 和多尺寸 `icon.ico`。

不迁移：Magpie、Locale Emulator、游戏启动注入、补丁、存档、云备份、资源商城、下载账号、旧 AI 剧情分析。原工作区已有的不完整 `src-tauri` 副本保存在忽略目录 `artifacts/legacy-tauri`，不参与构建。原始样本脚本、原图和抠图未改写。

## 后续阶段

IndexTTS 2.5 日语角色 LoRA、GPT-SoVITS v5Turbo 与 v2Pro 两阶段训练和 GalExtract 的 AdvHD 语音拆取已接入独立麦克风工作台。`core/voice` 定义工作进程协议，`src-tauri/voice` 管理 GPU 租约、角色专属输出与进度 Channel，`ui/voice` 保留独立草稿、请求及有界日志。语音任务写入角色档案内的 `voice/`，长期计算不持有库锁。任务完成或取消前，角色删除事务会拒绝删除正在写入的档案。基础模型与原游戏只读，语音、特征和权重保存在磁盘。详见 [声音训练](voice-training.md)。

桌面角色窗口通过 `core/companion`、`connections/chat`、`src-tauri/companion` 和独立的 `ui/companion` 连接资料、双语角色对白和已训练的日语 LoRA。窗口会话隔离，历史仅在内存，模型显式绑定；合成与训练共用 GPU 租约。详见 [桌面角色陪伴](desktop-companion.md)。其他引擎的自动语音映射仍属于后续阶段。

## 官方接口依据

- [Bangumi 公开 API](https://github.com/bangumi/api/blob/master/open-api/v0.yaml)
- [Bangumi 网站相册接口](https://github.com/bangumi/dev-docs/blob/master/api.yaml)
- [NextMoe 角色目录](https://developer.nextmoe.dev/docs/v2/listCatalogCharacters)
- [VNDB Kana](https://api.vndb.org/kana#post-character)

NextMoe 与 VNDB 返回可用数据的范围以其 API 为准；“完整保留”表示保留本次接口获取的所有字段和关联响应，不表示镜像整个站点数据库。

## 自动标签补全

默认标签源跟随界面语言，中文为鲲 Gal，英文和日文为 VNDB。设置页可显式指定全局来源，角色详情的数据库按钮可手动选取角色条目并为该角色覆盖默认来源。旧记录缺少 `tag_source_automatic` 时采用语言规则，缺少角色 `tag_source` 时跟随全局选择。

`core/sources/tag_lookup` 复用详情请求，使用 NextMoe 的 `refs=source:id` 批量查询和返回的外部 ID 定位角色，不凭姓名关联。一个明确身份关联多个 VNDB ID 时合并标签并保留每个完整快照。无法匹配、请求失败和空标签分别处理。`ui/tag_fetch` 在加载、导入和设置保存后检查缺失来源，后台串行补全，失败在详情标签区显示并可重试。同一会话每个角色和来源自动尝试一次，避免失败循环。

`src-tauri/character_tags` 在锁外请求网络，提交前核对来源与快照，复用原子保存短事务。自动任务完成后 UI 重读最新库，避免旧响应覆盖并发收藏或换图。手动添加的标签保存在 `tag_presentation.added`，与 API 原始标签分开，支持编辑、删除、排序和筛选。

## 情绪参考音频

v5Turbo 的聊天提示让 AI 随日语对白一起返回有序情绪段。core 验证段数、情绪和台词，合并相邻同情绪段，再从台词拼出展示用的日语原文。桌面端根据各段情绪解析本地参考，交给同一个 Python worker。worker 复用已加载模型逐段合成，校验后写出一条 WAV。

`core/companion/emotions` 提供五类情绪、候选、选择与页面协议，其 `storage` 从首次保存的素材来源读取台词并验证 WAV、文件边界和内容摘要。配置按角色独立保存，`source_dir` 与当前推理模型解耦并兼容旧 `model_dir` 字段；三个引擎通过 `voice/python/reference_audio.py` 校验和使用同一条已选参考。`core/connections/emotion_recommendations` 复用聊天服务的有界 JSON 请求，分批筛选再归并到每类最多 5 条。`src-tauri/companion/emotions` 管理主窗口权限、悬浮角色身份、取消和提交前复核。写入与角色删除共用库锁并检查未完成删除，网络请求在事务外运行。`ui/emotions` 管理独立请求及试听状态，`web/emotions.css` 使用既有主题变量。

## GalGear 游戏库联动

`core/game_library` 从用户指定的 `catalog.json`、数据目录或含 `portable.flag` 的便携目录读取轻量游戏投影，同时合并 `settings.json` 中的手动条目、资料覆盖与删除排除规则。使用经过主机和路径校验的鲲 Gal / NextMoe、Bangumi、VNDB 作品 ID，以及鲲 Gal 封面摘要进行自动匹配。读取当前 metadata、tag_source_url 和 cover_overrides，兼容 GalGear 的 source-<下载内容 SHA-256>.jpg 封面记录。多个出演作品共用的封面不作为独立匹配依据。损坏文件返回错误，读取不写回 GalGear。

出演作品仅从鲲 Gal 快照投影。鲲 Gal 角色资料获取通过明确角色 ID 查询 appearances，并批量保存作品 refs/covers 响应至 work_details。旧角色可在出演作品区手动刷新资料，提交核对原始快照并通过短事务保存。旧 game_links 保留序列化兼容，但不参与关联。标题经固定主机和数字 ID 校验打开鲲 Gal 作品页。

`ui/game_library` 启动时读取一次已配置的 GalGear 库，页面切换复用本次读取结果。无配置时跳过读取，读取失败或无匹配后等待下次启动、路径变更或主动刷新。模块独立跟踪请求并忽略迟到响应。训练页先选角色，再选关联游戏。切换素材清除旧映射及说话人。解包启动前由后端重新读取库并校验角色关联、目录可用性和选中路径，防止使用陈旧安装信息。

手动选择的目录按鲲 Gal 作品 ID 保存到当前角色的 `work_folders`，取消选择不产生写入。打开目录优先使用手动关联，否则从最近一次读取的 GalGear 库快照选取匹配安装。目录打开和手动关联的解包都在后端重新校验实际路径。

AI 解包通过 `voice_run` 的 `skill_extract` 动作启动独立 Python worker。`voice/agent.rs` 只读取 GalFriends 的 AI 凭据，通过标准输入传递，不写入日志或提示词。`skill_host.py` 为声音任务适配 GalGear 宿主：直接复用其 API 传输、固定归档工具、纯解析进程与结果限流，不运行剧情/存档任务构造器。声音工具只读游戏与技能，输出始终位于用户显式选择的新任务目录；路径拒绝重解析点。当前上游宿主支持 OpenAI、DeepSeek，其他服务明确提示不支持。

GalGear 宿主代码由 `GALFRIENDS_GALGEAR_HOST` 指向含 `agent_host.py` 的目录，开发环境默认读取同级 GalGear 源码。该适配依赖上游工具接口，升级宿主后需重测；不修改或自动打包上游代码。Windows Job 和任务租约管理全部子进程。最终结果校验音频边界、日语字段、来源 SHA-256 与消息/姓名/语音坐标。通用纯解析器另外要求每条文本、姓名、语音引用有完全匹配的字节跨度；结构证据不能独立替代引擎语义验证。未解决项始终保留在报告及工作台计数中。

表情自动发现与断点由 `artwork/expressions/discovery`、离线 `sprite_expressions/scan.py` 和原子角色目录清单承载；桌面 `expressions/tasks` 调度串行后台扫描，UI `expression_scans` 管理独立通知与暂停。详情页先展示游戏原始表情，仅确认后融合当前立绘。协议与恢复细节见 [表情工具](artwork-expressions.md)。
