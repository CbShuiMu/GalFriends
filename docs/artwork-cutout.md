# 主人物抠图

海报中的背景人脸、风景和 Logo 可能被 ISNet Anime 一起认作前景。仅提高 alpha 阈值无法区分这些对象，而且会损伤皮肤、衣服和发丝。应用入口改为 `character_foreground.py`，原 `character_art.py`、样本原图与 `keep.json` 保持原样。

## 处理规则

1. 保留原 ISNet Anime 蒙版。检测最可能的主要人物，优先完整、高置信、靠近画面中部的人物，减少背景头像干扰。
2. 收窄检测框作为 SAM 2 的提示，配合框内上身点选择人物。提示框不裁切画布，框外仍能保留头发。模型置信度只用于实例选择，不直接作为皮肤或衣服的透明度。
3. 若原蒙版与人物实例接近，保留原蒙版，避免再次损伤简单背景的发丝。复杂背景取两个蒙版的交集，不允许新模型重新引入 ISNet 已删除的背景。
4. 仅当下部出现两个高置信人物区域、之间仍有低置信前景时，补充局部背景点，清理腿间文字。没有双腿间隙的长裙、并腿不强制添加背景点；局部修正不改头部或上身。
5. 清理孤立碎片和细桥连接的文字；保留所有足够大的连通区域，不能只保留最大区域，否则会误删与长裙分离的鞋子。边缘继续用原 alpha 38–225 曲线。已有人工 keep 修正的样本沿用原算法。
6. 输出仍是同画布透明 PNG，前景 RGB 取自原始图。然后执行 waifu2x 2× 超分，再统一提交角色库和历史归档。

这是针对立绘的自动选择策略，不保证任意多人合照、极端姿势或与身体粘连的装饰都能完全分离。主人物选错、低清发丝和真实遮挡仍需人工检查；不通过生成内容补画被遮挡的身体。没有人物检测结果时沿用原 ISNet，缺少模型或输出无效则报错。

## 模型和环境

模型均只在安装阶段下载到 `%LOCALAPPDATA%/GalFriends/models/`。原 ISNet 约 176 MB；新增 ONNX 权重合计约 372 MB。没有新增 Python 包，沿用 NumPy、Pillow、ONNX Runtime 和 requests。运行期间不联网下载模型、不上传图片。CPU 推理每张的时间取决于硬件；core 对单张抠图、超分各保留 120 秒超时。

| 模型 | 固定版本 | 来源与许可 |
| --- | --- | --- |
| ISNet Anime | 原脚本的 SHA-256 | [rembg ISNet Anime](https://github.com/danielgatis/rembg/blob/main/rembg/sessions/dis_anime.py) |
| SAM 2.1 Hiera Base Plus ONNX | `bab18593f44e652f04cf18b60b3690f60e8996b0` | [ONNX Community 权重](https://huggingface.co/onnx-community/sam2.1-hiera-base-plus-ONNX)，[Meta SAM 2 / Apache-2.0](https://github.com/facebookresearch/sam2) |
| Anime person v1.1 s ONNX | `e39c744c22432ad01f91dd254fe2b02c8d878b8c` | [DeepGHS 权重及模型卡](https://huggingface.co/deepghs/anime_person_detection)，模型卡标注 MIT |

逐文件 URL 和 SHA-256 的唯一清单是根目录 `character_foreground.models.json`。准备命令为 `scripts/setup-cutout.ps1`；原 `character_art.py cutout ...` 仍可独立使用原模型。

## 验证

```powershell
.venv\Scripts\python.exe -B -m unittest discover -s tests -p test_character_foreground.py
cargo test --workspace --locked
cargo run -p galfriends-core --example verify_import_artwork --locked -- target/debug/tools/waifu2x
```

离线蒙版测试覆盖分离鞋子保留、文字碎片去除、长裙/干净间隙不添加背景点、原 RGB 和透明输入保留。`verify_import_artwork` 在临时库验证原图保留、已有人工修正样本不变、真实 waifu2x 和归档往返。复杂背景还需要用合法持有的 Bangumi 原图检查头发、服装、鞋子及背景文字；测试图不作为新增发行素材。
