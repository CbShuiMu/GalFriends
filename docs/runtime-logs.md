# 运行日志

日志系统移植自 GalGear。GalFriends 在加载角色库、创建窗口之前，在 **GalFriends.exe 所在目录的 `logs/`** 创建本次会话文件，例如 `galfriends_2026-11-16_143025_123.log`。文件名采用启动时的本地时间（毫秒）；重名时追加序号，不覆盖已有文件。日志位置不随工作目录改变，不访问 GalGear 的日志或配置。日志由用户自行删除，没有自动轮转、清理或上传。

文件采用 UTF-8 JSON Lines，每行一条记录。普通记录包含带时区的时间、启动后毫秒数、级别、进程、线程、模块和消息。直接写入文件，ERROR 额外刷盘；正常退出也刷盘。

## 记录范围

- `session_start`、`desktop_setup`、`desktop_ready`：版本、系统、架构、可执行文件和目录、可用 WebView2 版本与启动进度。角色库加载错误单独记录，仍由原有界面提供重试。
- `command_start`、`command_ok`、`command_error`：主窗口和桌面角色窗口的命令名称、请求关联 ID、耗时与失败原因。前端结果不一定送达；只有开始记录不能推断业务成功或失败。
- `background_start`、`background_ok`、`background_error`：现有后台阻塞任务在原生侧记录结果。导入、图片处理、资料更新、服务连接、训练和角色对话等任务即使失去前端，也能保留完成或失败线索。原有任务、锁、取消与事务机制不变。
- `frontend_ready`、`view_applied`、`view_frames`、`view_frames_delayed`、`frontend_visibility`：WASM 初始化、预定义页面/弹窗类别和帧回调进度。两次帧回调只证明浏览器仍在处理帧，不能证明屏幕像素已经呈现；同一页面重绘不会反复记录切换。
- `javascript_error`、`wasm_panic`、`galfriends::panic`：脚本、资源加载、未处理 Promise、WASM panic，以及 Rust panic 的位置和堆栈。
- `window_close_requested`、`exit_requested`、`shutdown_start`、`session_end`：退出流程。会话结束包含 `webview_process_failed`，表示本次是否观察到 WebView2 故障。

## 原生故障

主窗口与每个桌面角色窗口都注册 WebView2 原生进程监听。`webview_diagnostics_ready` 保存实际运行时版本、浏览器 PID 和程序路径；`webview_process_failed` 记录窗口、故障类型、原因码、十进制/十六进制退出码及缓存的运行时信息，返回前刷盘。浏览器 PID 与日志顶层的 GalFriends PID 不同。

`webview_failure_report_folder` 记录运行时报告的转储目录；旧运行时不支持该接口时只记录警告，不撤销故障监听。GalFriends 不创建、复制或上传转储。主页面或浏览器进程退出后，每个受影响窗口异步显示一次原生提示，不自动退出、重启或中断写入；GPU/子框架故障与渲染无响应只记录。

Windows 未处理异常使用预打开的独立追加句柄和栈上缓冲，尽力记录异常码、地址、模块基址、线程和本地时间，不使用普通日志锁，不阻止系统处理异常。Rust 堆栈的符号完整性取决于构建符号。断电、强杀、内存耗尽、严重栈损坏、fast-fail 等仍可能没有故障尾记录，不能仅凭缺少 `session_end` 确定根因。

## 隐私与失败处理

不记录命令参数、成功响应正文、完整配置、图片、音频、输入、AI 对话、请求头或请求体。页面诊断只接受枚举类别，不读取标题、角色姓名、页面内容或截图。普通日志只接受 GalFriends 自身的 INFO/WARN/ERROR，不开启第三方 HTTP 调试日志。

写入前统一脱敏 URL、内嵌资源、常见凭据字段/认证头、Windows 用户目录用户名和邮箱；保留本地前端资源名及堆栈行列号，普通消息限制为 32768 个字符。调用方仍必须避免主动传入私密正文，脱敏不是记录全部数据的许可。分享日志前应检查剩余的本地路径与错误上下文。

创建日志失败时原生提示并继续启动，不静默写到其他位置；运行中写入失败只异步提示一次。日志 IPC 失败不会改变业务命令结果，也不会递归上报自身故障。

## 实现与验证

`src-tauri/src/diagnostics.rs` 负责会话、命令与后台任务边界；`diagnostics/privacy.rs` 负责脱敏；`diagnostics/windows.rs`、`diagnostics/webview.rs` 分别处理宿主异常和 WebView2。`diagnostics/frontend.js` 由 Tauri 插件在页面脚本之前注入全部窗口，因此无需复制到 `dist/`。两个页面桥接均经过 `GalFriendsDiagnostics.invoke`；Rust UI 的两个启动入口安装 panic hook。core 不依赖桌面日志模块。

```powershell
cargo test -p galfriends --locked diagnostics
node --test scripts/test-diagnostics.mjs
# 两个测试分别独立运行，仅让临时配置中的测试 WebView 崩溃。
cargo test -p galfriends --locked diagnostics::tests::native_webview_records_frontend_view_and_renderer_failure -- --ignored --exact --nocapture
$env:GALFRIENDS_TEST_BROWSER_CRASH = '1'
try {
    cargo test -p galfriends --locked diagnostics::tests::native_webview_records_frontend_view_and_renderer_failure -- --ignored --exact --nocapture
} finally { Remove-Item Env:GALFRIENDS_TEST_BROWSER_CRASH }
```

Rust panic 和 Windows 异常测试使用独立子进程及临时日志目录。WebView2 测试只注册诊断命令，使用隐藏窗口与独立临时 profile，不加载或修改真实角色库。常规自动化不触发原生提示窗口，不请求真实服务或训练模型。
