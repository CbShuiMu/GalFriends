//! 自动标签补全与手动替换。网络在锁外，提交核对身份、来源和快照，复用角色库短事务。
use crate::commands::{AppState, read, update};
use galfriends_core::{
    characters::{Library, Source},
    sources, tags,
};
use tauri::Manager;

#[tauri::command]
pub(crate) async fn update_character_tags(
    app: tauri::AppHandle,
    id: String,
    source: Source,
    source_id: Option<String>,
) -> Result<Library, String> {
    if !matches!(source, Source::Kungal | Source::Vndb) {
        return Err("Unsupported tag source".into());
    }
    let library = read(&app.state::<AppState>())?;
    let expected = library
        .characters
        .iter()
        .find(|c| c.id == id)
        .ok_or("Character not found")?;
    let expected_snapshots = expected.snapshots.clone();
    let expected_source = expected.tag_source;
    let settings = library.settings.clone();
    drop(library);
    let credentials = app.state::<AppState>().credentials_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let client =
            |source| crate::connections::catalog_client(&credentials, &settings.proxy, source);
        let result = if let Some(source_id) = &source_id {
            sources::fetch(&client(source)?, source, source_id)?
        } else {
            sources::tag_lookup::fetch_for(&expected_snapshots, source, client)?.ok_or_else(
                || {
                    galfriends_i18n::tr(
                        galfriends_i18n::Language::from_code(&settings.language)
                            .unwrap_or_default(),
                        "tags.no_match",
                    )
                    .to_owned()
                },
            )?
        };
        update(&app.state::<AppState>(), |library| {
            let default = library.settings.effective_tag_source();
            let current = library
                .characters
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or("Character removed during tag lookup")?;
            if current.snapshots != expected_snapshots
                || current.tag_source != expected_source
                || (source_id.is_none() && tags::source(current, default) != source)
            {
                return Err("Character source changed during tag lookup".into());
            }
            current.merge(result);
            if source_id.is_some() {
                current.tag_source = Some(source);
            }
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
