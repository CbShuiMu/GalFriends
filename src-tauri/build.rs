fn main() {
    // 图标内容变化时，重新生成 Tauri 的窗口和可执行文件图标资源。
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");
    tauri_build::build();
}
