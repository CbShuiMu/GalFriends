use wasm_bindgen::prelude::wasm_bindgen;
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render(html: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render_theme(html: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render_import_progress(id: u32, text: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn render_voice_progress(id: u32, text: &str);
    #[wasm_bindgen(js_namespace=GalFriendsBridge)]
    pub(crate) fn request(id: u32, command: &str, payload: &str);
}
