# 离线立绘表情提取

`character_expressions.py` 使用 GalFriends 已保存的立绘，从 GARbro 导出的 PNA 容器中匹配同姿势身体并导出原生表情层。通用逻辑位于 `crates/core/src/artwork/python/sprite_expressions/`，入口只负责调用命令行模块。

本版支持 PNAP 容器内的 PNG 图层，已用《Nightmare×Sisters》的神堂刹那验证。匹配依据为头部局部 SIFT 特征及 RANSAC 相似变换，允许服装、缩放和画布留白不同。它没有人体骨骼检测模型，头发、角度或画风变化很大时可能无匹配。头部相同但手臂不同的素材需要查看 `alignment.jpg` 确认姿势。

## 准备与运行

使用项目 Python 环境安装独立依赖。Pillow、NumPy 沿用项目版本，新增 OpenCV CPU 包提供 SIFT 和几何匹配。依赖仅用于离线工具，约 40 MB 的 Windows wheel 不加入桌面或 WASM 构建。OpenCV 使用 Apache-2.0 许可，Python 包内同时提供第三方许可证。

```powershell
.venv/Scripts/python.exe -m pip install -r requirements-expressions.txt
```

在 GARbro 中打开游戏的 `Chip4.arc`，将其中的 `.pna` 文件导出到独立目录。保留 PNA 可以同时读取图层坐标、空记录及原始负载。以本次本地样本为例，在仓库根目录运行下面的命令。

```powershell
.venv/Scripts/python.exe character_expressions.py `
  --library artifacts/characters/library.json `
  --character bangumi:76896 `
  --assets-root dist `
  --source artifacts/expression-study/Chip4 `
  --base-index 16
```

`--library` 支持当前 v2 轻量索引及旧内嵌角色库。v2 读取当前索引的 `selected_art`，核对归档身份和内容摘要。`--character` 必须是明确 ID。图片优先读取所选立绘的原尺寸透明 `url`，避免超分纹理改变匹配，支持内嵌 data URL。旧库使用 `assets/...` 时，`--assets-root` 必须指向包含该目录的本地根目录。

本次选择的立绘为下标 3、图片 ID `21287`，来自《vampire》的制服，与《Nightmare×Sisters》的制服不同。命令中的 `--base-index 16` 选择游戏的完整制服身体层用于匹配和合成预览，此值属于本次样本。其他容器可以省略该参数，由特征证据选择身体候选，也可以根据清单明确指定记录号。每个身体候选均记录匹配点、内点比例与坐标变换，未通过门槛的候选不会导出。

其他角色可以直接指定本地透明立绘，也可以通过 `--artwork-index` 覆盖库内的当前选择用于单次测试。

```powershell
.venv/Scripts/python.exe character_expressions.py `
  --reference path/to/reference.png `
  --source path/to/exported-pna `
  --output path/to/results
```

`--head-fraction` 为参考图前景顶部参与匹配的比例，默认 `0.3`，范围 `0.1–0.6`。候选使用顶部 `0.35`，可容纳游戏半身图和全身参考的差异。自动门槛要求至少 8 个独立位置内点、45% 内点比例、旋转不超过 8 度，并检查匹配点的二维分布。阈值是当前样本及合成测试上的工程取值，跨作品结果需查看证据。

## 产物与验证

使用角色库时不需要选择保存目录。任务自动存放在 `characters/<角色目录>/expressions/runs/expressions-随机值/`，完整成功后自动归档素材包并原子发布 `expressions/catalog.json`。原始 PNG、偏移、身体层和来源清单一并保留，角色库索引与资料版本不会被工具改写。可选 `--output` 只覆盖审计产物的位置，仍自动归档到对应角色。直接使用 `--reference` 的独立模式必须指定输出目录，不自动关联角色。

除软件计算的角色专属任务目录外，输出目录与输入目录需互不包含，扫描拒绝符号链接和 Windows 重解析点。Ctrl+C 会把已创建任务标为 `interrupted`，再次运行生成新目录。部分失败与零匹配不发布到角色目录索引。素材复制校验后才发布，重复归档相同素材不新增条目，文件锁防止并发覆盖；损坏索引或图片会明确报错。

| 文件 | 内容 |
| --- | --- |
| `report.json` | 当前参考来源、参数、全部候选及接受/拒绝证据、成功导出与失败项 |
| `reference.png` | 本次从角色库读取的参考图副本 |
| `000/manifest.json` 等 | 对应源 PNA、SHA-256、画布、身体记录号、完整记录表、表情偏移和验证结果 |
| `000/original/` | PNA 内 PNG 负载的原始字节，包括选定身体层 |
| `000/expressions/` | 恢复预乘 RGB 后的透明 PNG 表情层，保留原图尺寸和 alpha |
| `000/base.png` | 放到原画布中的身体层 |
| `000/previews/` | 身体与单个局部图层按原坐标进行 source-over 合成的预览 |
| `000/alignment.jpg` | 从左到右为当前参考、几何对齐的身体和叠加图 |
| `000/contact-sheet.jpg` | 各局部图层在游戏身体上的脸部预览 |

贴回时在 `base.png` 画布上，将 `expressions/` 图片放在清单的 `offset=[x,y]`，使用标准 alpha 合成。原始 PNA PNG 的 RGB 已预乘 alpha，直接使用 `original/` 中的图像可能出现暗边。恢复规则遵循 GARbro 的整数除法。格式布局和颜色处理依据见 [GARbro PNA 读取器](https://github.com/morkt/GARbro/blob/master/ArcFormats/Will/ArcPNA.cs)，匹配依据见 [OpenCV 特征匹配文档](https://docs.opencv.org/4.13.0/dc/dc3/tutorial_py_matcher.html)。

完整表情与腮红等辅助层可能属于同一容器组。工具保留源记录号，不推断喜怒哀乐标签或游戏脚本组合。某个辅助层的单层预览没有眼口时，可以在实际使用中将其与完整表情共同合成。本次 `001–006` 为完整表情，`007` 为腮红辅助层。

工具将每个输出 PNG 重新读取并按清单偏移回贴，检查 RGBA 逐像素一致，同时核对原始 PNG 字节、解码后的像素和输入 PNA 内容摘要。该验证覆盖导出和坐标往返；具体脚本中的叠层顺序仍需引擎适配。结果中 `status=complete` 才返回退出码 0，损坏资源或部分失败返回 2 并保留失败记录，零匹配返回 2，用户中断返回 130。

## 本地样本结果

2026-11-17 扫描 `Chip4.arc` 导出的 12 个 PNA，跨服装匹配命中 `ST01A_L` 和 `ST01A_M`。两种尺寸各导出 6 张完整表情与 1 张辅助层，共 14 张，所有落盘回贴差异为 0。另一种姿势 `ST01B` 和其他角色未通过几何验证。合成预览显式选择完整制服身体第 16 层。

产物为本地验证资源，放在忽略的 `artifacts/` 下。Bangumi 参考来源保存在报告的 `reference.original`，游戏素材来源保存在各清单中。样本使用范围沿用原素材约定。

```powershell
.venv/Scripts/python.exe -m unittest discover -s tests -p test_character_expressions.py -v
```

测试覆盖旧库与 v2 选择优先级、归档摘要、PNA 截断与越界、alpha 恢复、原始图层保留、坐标回贴、跨服装缩放匹配、无关图拒绝及部分失败记录。其他引擎格式与仅有扁平完整立绘的像素差分尚未实现。

## 当前立绘表情替换

详情页立绘右上角的 face 图标直接打开表情选择弹窗，只读取该角色素材包中的 `choices.json` 与 `choices/*.png`。这些是提取/归档阶段保存的游戏原始表情近景，不含当前立绘融合结果。打开时会独立检查关联的本地游戏并在后台提取缺失素材；浏览和取消不会将表情融合到当前立绘。已保存的原始缩略图立即显示，无需等待扫描。

用户选择并点击“确认并替换表情”后，后台才核对当前立绘、校验所选素材包、匹配姿势，仅合成所选的一项并保存。不同素材文件的选项标出来源，同一表情的 L/M 等尺寸版本合并为一个选项，优先使用原生分辨率更高的版本。不会在确认前合成目标立绘。无素材显示空状态，确认后若姿势不匹配则报错并保留选择，原立绘不变。

结果追加为派生立绘并选中，首页、角色库及已打开的桌面角色窗口同步显示。原图可从相册切回。选择窗口打开后若立绘已切换或重新处理，确认时会报错并保留窗口，需重新打开选择窗口。删除角色时，差分、清单和预览随角色专属目录一起清理，不删除外部导出原件或游戏文件。

已有离线提取结果可一次性归档，随后 face 按钮即可读取；参数只指定输入清单，保存位置由软件确定。旧版素材缺少 `choices.json` 时重复运行归档命令即可补齐原始缩略图：同一 PNA/身体记录更新索引，旧包和原始导出保留，不在打开 face 时临时生成。本地刹那素材已完成此迁移。

```powershell
.venv/Scripts/python.exe character_expressions.py `
  --character bangumi:76896 --assets-root dist `
  --import-manifest artifacts/expression-results/expressions-tx6fz31p/000/manifest.json `
  --import-manifest artifacts/expression-results/expressions-tx6fz31p/001/manifest.json
```

运行时使用项目 `.venv` 和根目录的 Python 入口，与现有开发预览的图片处理环境一致。需先安装 `requirements-expressions.txt`。独立发行包的 Python 运行时仍待打包。

离线合成默认从仓库 `characters/library.json`，或开发运行目录 `artifacts/characters/library.json` 读取当前已保存的立绘选择。多角色库通过 `--character` 明确指定角色身份，也可显式传入 `--library`。旧库的相对素材路径仍需 `--assets-root`。

```powershell
.venv/Scripts/python.exe character_expressions.py --compose `
  --character bangumi:76896 --assets-root dist
```

合成优先使用当前立绘的超分图作为输出底图，按原尺寸透明图匹配。每次重新估计相似变换，统一清理所有表情覆盖的旧眼口，使用游戏无表情身体的面部底色和表情层恢复五官，边缘羽化。面部遮罩内部完整替换，遮罩外的 RGBA 和整图透明通道逐像素保持。遮罩触及人物透明边缘时拒绝合成。

默认把高度不足完整表情 60% 的局部层作为辅助层，与最后一个完整表情组合，并将辅助层放在五官后面。此判断依据素材几何信息，预览需要人工确认。`--variant 7,6` 可以指定从底到顶的组合，重复该参数可输出多个组合。`--reference` 配合可选的 `--display` 支持独立图片输入。

CLI 的显式 `--compose` 命令从该角色归档中自动选择匹配素材，结果保存到角色 `expressions/previews/composed-随机值`，可用 `--manifest` 和 `--output` 覆盖以进行独立调试。产物包含当前参考图、透明 PNG、脸部对照、面部遮罩及验证清单。桌面确认命令通过 `--catalog`、`--pack` 和单个 `--variant` 精确使用用户所选素材，在临时目录生成一项结果，再用已有库原子保存事务提交。提交前再次校验源立绘和当前选择，失败保留选择和旧库，同一合成结果不会增加重复图片。


## 自动发现、断点与唯一表情

首次点开 face，在角色 `expressions/discovery.json` 原子保存启用标记，随后后台读取最新的 GalGear 只读游戏库关联与角色手动关联目录。关联基于作品 ID/既有封面身份规则，不凭名称猜测。规范化安装目录去重；每次打开均重新读取关联，已完成目录跳过，仅扫描新增目录与上次失败项。尚未点击过 face 的角色不自动启动扫描。

扫描沿用本地 GARbro Mod，只列归档目录并选择性导出 `.pna` 成员；目前自动解析的是已验证的 PNAP/PNG 分层立绘。其他格式显示未发现受支持素材，不伪称已提取。游戏文件只读，素材、原始来源和断点归角色目录所有。读取角色已保存的原始立绘进行头部匹配，不依赖当前服装相同；不通过同名自动认定角色。

断点记录归档清单、大小/修改时间、当前归档和成员游标。每个成员先完整发布素材，再原子推进游标；退出发生在两者之间时，重复归档是幂等的。下次启动自动恢复未完成任务，只重做中断中的成员。损坏或暂不可用的资源记录失败并保留进度，后续重新打开或启动会重试；资源在处理中改变时拒绝提交。

任务与选择弹窗独立，关闭弹窗仍继续。右下角共用通知展示阶段与结果，表情窗口提供暂停入口。暂停保留断点，下次打开或重启继续。退出时终止扫描子进程及其 GARbro 后代；异常退出由父进程守卫处理。删除角色前需暂停其任务，避免清理后被后台重新创建。单应用同时执行一个扫描。

归档阶段基于原生脸部图层生成统一尺度的像素描述，包含辅助层（例如腮红）。只有互为最佳、与其他表情有明显距离的近似匹配才合并，不按文件名、图层序号或表情标签去重。唯一索引 `catalog.json/unique` 保持稳定身份并选取最高原生分辨率；原始包及来源关系保留供追溯，原始游戏文件不删除。已有目录在后台自动迁移；刹那的 L/M 共 14 个选项已验证合并为 7 个，保留腮红差分。保守去重对于画法明显不同的重制资源仍可能保留为不同表情。

后台入口为 `crates/core/src/artwork/python/scan_worker.py`，复用同一离线提取和归档包；Tauri 仅负责调度、取消和事件，core 负责关联目录与进程边界。`crates/core/src/python/worker_guard.py` 由表情和语音任务共享。当前开发预览依赖项目 `.venv`、GARbro CLI 与适配助手，独立发行环境尚未打包。
