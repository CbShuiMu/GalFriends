use wasm_bindgen::prelude::wasm_bindgen;
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    fn report_panic(message: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render(html: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render_theme(html: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render_import_progress(id: u32, text: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render_voice_progress(id: u32, text: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render_voice_log(id: u32, text: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render_update_progress(id: u32, text: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn request(id: u32, command: &str, payload: &str);
}

pub(crate) fn install_panic_hook() {
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|panic| report_panic(&panic.to_string())));
}
