//! 网络请求在后台线程执行；短事务串行保存，成功后才发布新内存状态。
use galfriends_core::{
    characters::{Candidate, FavoriteUpdate, ImportProgress, Library, Source},
    library_ops::{self, BatchOperation},
    sources, store,
};
use std::{path::PathBuf, sync::Mutex};
use tauri::Manager;
pub(crate) struct AppState {
    pub path: PathBuf,
    pub credentials_path: PathBuf,
    pub library: Mutex<Result<Library, String>>,
}
/// 收藏返回已提交字段；其余通用操作沿用完整库响应。
#[derive(serde::Serialize)]
#[serde(untagged)]
pub(crate) enum MutationResult {
    Favorites(FavoriteUpdate),
    Library(Box<Library>),
}
pub(crate) fn read(s: &AppState) -> Result<Library, String> {
    s.library.lock().map_err(|e| e.to_string())?.clone()
}

#[tauri::command]
pub(crate) async fn enhance_artwork(app: tauri::AppHandle, id: String) -> Result<Library, String> {
    let l = read(&app.state::<AppState>())?;
    let c = l
        .characters
        .iter()
        .find(|c| c.id == id)
        .ok_or("Character not found")?;
    let art = c.artwork().ok_or("No artwork")?.clone();
    let art_id = art.id.clone();
    let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let engine =
        crate::preferences::waifu2x_directory(&resources).ok_or("waifu2x not installed")?;
    crate::diagnostics::spawn_background("enhance_artwork", move || {
        let transparent = if let Some(bytes) = galfriends_core::artwork::png_data(&art.url)? {
            Some(bytes)
        } else if let Some(file) = art.url.strip_prefix("assets/setsuna/") {
            if !file.ends_with(".png")
                || !file
                    .trim_end_matches(".png")
                    .bytes()
                    .all(|c| c.is_ascii_digit())
            {
                return Err("Invalid sample path".into());
            }
            let path = [
                resources.join("sample").join(file),
                dev.join("web/assets/setsuna").join(file),
            ]
            .into_iter()
            .find(|p| p.is_file())
            .ok_or("Sample not installed")?;
            Some(std::fs::read(path).map_err(|e| e.to_string())?)
        } else {
            None
        };
        let prepared = if let Some(bytes) = transparent {
            let mut prepared = art.clone();
            prepared.enhanced = Some(galfriends_core::artwork::enhance(&engine, &bytes)?);
            prepared
        } else {
            // 旧的远程记录也走同一去背景链，不能用有背景的原图覆盖透明超分结果。
            let bytes = galfriends_core::artwork::download(&l.settings.proxy, &art.original)?;
            let cutout = crate::artwork::cutout_engine(&resources)?;
            galfriends_core::artwork::prepare(&cutout, &engine, &art, &bytes)?
        };
        update(&app.state::<AppState>(), |l| {
            let c = l
                .characters
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or("Character removed during enhancement")?;
            let target = c
                .artworks
                .iter_mut()
                .find(|a| a.id == art_id && a.original == art.original)
                .ok_or("Artwork changed during enhancement")?;
            galfriends_core::artwork::replace_processed(
                std::slice::from_mut(target),
                std::slice::from_ref(&art),
                vec![prepared],
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
pub(crate) fn update(
    s: &AppState,
    f: impl FnOnce(&mut Library) -> Result<(), String>,
) -> Result<Library, String> {
    // 原子保存与内存发布组成单个短事务，防止并发收藏操作覆盖彼此。
    let mut guard = s.library.lock().map_err(|e| e.to_string())?;
    let mut next = guard.clone()?;
    f(&mut next)?;
    if !next.settings.library_order.custom_order.is_empty()
        || next.settings.library_order.sort == galfriends_core::library_order::Sort::Custom
    {
        galfriends_core::library_order::reconcile(
            &mut next.settings.library_order,
            &next.characters,
        );
    }
    store::save_changes(&s.path, guard.as_ref().map_err(Clone::clone)?, &next)?;
    *guard = Ok(next.clone());
    Ok(next)
}
#[tauri::command]
pub(crate) async fn load_library(app: tauri::AppHandle) -> Result<Library, String> {
    crate::diagnostics::spawn_background("load_library", move || {
        let state = app.state::<AppState>();
        if !state.path.exists() {
            update(&state, |_| Ok(()))
        } else {
            let mut guard = state.library.lock().map_err(|e| e.to_string())?;
            let library = store::load(&state.path)?;
            *guard = Ok(library.clone());
            Ok(library)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn search_characters(
    app: tauri::AppHandle,
    source: Source,
    query: String,
) -> Result<Vec<Candidate>, String> {
    let proxy = read(&app.state::<AppState>())?.settings.proxy;
    let path = app.state::<AppState>().credentials_path.clone();
    crate::diagnostics::spawn_background("search_characters", move || {
        sources::search(
            &crate::connections::catalog_client(&path, &proxy, source)?,
            source,
            &query,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn import_character(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    source: Source,
    id: String,
    target: Option<String>,
    on_progress: Option<tauri::ipc::JavaScriptChannelId>,
) -> Result<Library, String> {
    let on_progress = on_progress.map(|id| id.channel_on::<_, ImportProgress>(webview));
    let library = read(&app.state::<AppState>())?;
    let needs_artwork = if let Some(target) = &target {
        library
            .characters
            .iter()
            .find(|c| &c.id == target)
            .ok_or("Character no longer exists")?
            .artworks
            .is_empty()
    } else {
        if library
            .characters
            .iter()
            .any(|c| c.snapshots.iter().any(|s| s.source == source && s.id == id))
        {
            return Ok(library);
        }
        true
    };
    let proxy = library.settings.proxy;
    let language =
        galfriends_i18n::Language::from_code(&library.settings.language).unwrap_or_default();
    let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
    let path = app.state::<AppState>().credentials_path.clone();
    crate::diagnostics::spawn_background("import_character", move || {
        let progress = |stage| {
            if let Some(channel) = &on_progress {
                // 窗口关闭或旧前端不接收通知时，仍完成原子导入；结果由命令响应确认。
                let _ = channel.send(stage);
            }
        };
        progress(ImportProgress::Importing);
        let c = sources::fetch(
            &crate::connections::catalog_client(&path, &proxy, source)?,
            source,
            &id,
        )?;
        let c = if needs_artwork {
            crate::artwork::prepare_character(c, &resources, &proxy, progress).map_err(
                |detail| {
                    galfriends_i18n::format_text(
                        language,
                        "error.artwork_import",
                        &[("detail", &detail)],
                    )
                },
            )?
        } else {
            c
        };
        progress(ImportProgress::Saving);
        update(&app.state::<AppState>(), |l| {
            if let Some(target) = target {
                let existing = l
                    .characters
                    .iter_mut()
                    .find(|c| c.id == target)
                    .ok_or("Character no longer exists")?;
                existing.merge(c);
            } else if !l.characters.iter().any(|old| {
                old.id == c.id
                    || old
                        .snapshots
                        .iter()
                        .any(|s| s.source == source && s.id == id)
            }) {
                l.characters.push(c);
            }
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn mutate_library(
    app: tauri::AppHandle,
    action: String,
    id: String,
    value: String,
) -> Result<MutationResult, String> {
    let display_changed = matches!(
        action.as_str(),
        "theme" | "color_theme" | "language" | "font_size"
    );
    let handle = app.clone();
    crate::diagnostics::spawn_background("mutate_library", move || {
        if action == "favorite" {
            let state = app.state::<AppState>();
            let mut guard = state.library.lock().map_err(|e| e.to_string())?;
            let library = guard.as_mut().map_err(|e| e.clone())?;
            let favorite = !library
                .characters
                .iter()
                .find(|c| c.id == id)
                .ok_or("Character not found")?
                .favorite;
            return store::set_favorites(&state.path, library, &[id], favorite)
                .map(MutationResult::Favorites);
        }
        // 兼容旧桥接调用，统一走轻量选择事务。
        if action == "art" {
            let index = value.parse::<usize>().map_err(|e| e.to_string())?;
            let state = app.state::<AppState>();
            let mut guard = state.library.lock().map_err(|e| e.to_string())?;
            let library = guard.as_mut().map_err(|e| e.clone())?;
            store::select_artwork(&state.path, library, &id, index)?;
            return Ok(MutationResult::Library(Box::new(library.clone())));
        }
        // 旧命令保留整库返回协议，但主题持久化规则只维护一份。
        if matches!(action.as_str(), "theme" | "color_theme") {
            let state = app.state::<AppState>();
            let mut guard = state.library.lock().map_err(|e| e.to_string())?;
            let library = guard.as_mut().map_err(|e| e.clone())?;
            store::set_appearance(&state.path, library, &action, &value)?;
            return Ok(MutationResult::Library(Box::new(library.clone())));
        }
        let game_library_path = if action == "galgear_library_path" && !value.trim().is_empty() {
            Some(galfriends_core::game_library::load(std::path::Path::new(value.trim()))?.path)
        } else {
            None
        };
        update(&app.state::<AppState>(), |l| {
            match action.as_str() {
                "galgear_library_path" => {
                    l.settings.galgear_library_path = game_library_path.unwrap_or_default()
                }
                "tag-add" | "tag-rename" | "tag-hide" | "tag-reorder" | "tag-reset" => {
                    let c = l
                        .characters
                        .iter_mut()
                        .find(|c| c.id == id)
                        .ok_or("Character not found")?;
                    galfriends_core::tags::apply(c, &action, &value)?;
                }
                "language" | "tag_source" | "search_source" | "font_size" | "ai_provider" => {
                    galfriends_core::preferences::set(&mut l.settings, &action, &value)?
                }
                "proxy" => {
                    if value.len() > 300 {
                        return Err("Proxy too long".into());
                    }
                    sources::client(&value)?;
                    l.settings.proxy = value;
                }
                "release_page" => {
                    l.settings.release_page = galfriends_core::updates::release_page(&value)
                        .map_err(|error| error.key)?;
                }
                "remove" => {
                    app.state::<crate::expressions::tasks::Tasks>()
                        .ensure_can_remove(std::slice::from_ref(&id))?;
                    app.state::<crate::voice::Tasks>()
                        .ensure_can_remove(std::slice::from_ref(&id))?;
                    library_ops::apply_batch(l, BatchOperation::Remove, &[id], "")?;
                }
                "collection" => {
                    galfriends_core::collections::save(l, None, &value, "")?;
                }
                "collect" => {
                    if !l.characters.iter().any(|c| c.id == id) {
                        return Err("Character not found".into());
                    }
                    if !galfriends_core::collections::accepts_characters(l, &value)
                        && !l
                            .collections
                            .get(&value)
                            .is_some_and(|ids| ids.contains(&id))
                    {
                        return Err("Choose a leaf collection".into());
                    }
                    let ids = l
                        .collections
                        .get_mut(&value)
                        .ok_or("Collection not found")?;
                    if ids.contains(&id) {
                        ids.retain(|v| v != &id);
                    } else {
                        ids.push(id);
                    }
                }
                _ => return Err("Unsupported action".into()),
            }
            Ok(())
        })
        .map(|library| MutationResult::Library(Box::new(library)))
    })
    .await
    .map_err(|e| e.to_string())?
    .inspect(|_| {
        if display_changed {
            crate::preferences::broadcast_display_preferences(&handle);
        }
    })
}
#[tauri::command]
pub(crate) async fn batch_characters(
    app: tauri::AppHandle,
    operation: BatchOperation,
    ids: Vec<String>,
    collection: String,
) -> Result<MutationResult, String> {
    crate::diagnostics::spawn_background("batch_characters", move || {
        if matches!(
            operation,
            BatchOperation::Favorite | BatchOperation::Unfavorite
        ) {
            let state = app.state::<AppState>();
            let mut guard = state.library.lock().map_err(|e| e.to_string())?;
            let library = guard.as_mut().map_err(|e| e.clone())?;
            return store::set_favorites(
                &state.path,
                library,
                &ids,
                matches!(operation, BatchOperation::Favorite),
            )
            .map(MutationResult::Favorites);
        }
        update(&app.state::<AppState>(), |l| {
            if matches!(operation, BatchOperation::Remove) {
                app.state::<crate::expressions::tasks::Tasks>()
                    .ensure_can_remove(&ids)?;
                app.state::<crate::voice::Tasks>().ensure_can_remove(&ids)?;
            }
            library_ops::apply_batch(l, operation, &ids, &collection)
        })
        .map(|library| MutationResult::Library(Box::new(library)))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn save_collection(
    app: tauri::AppHandle,
    original: Option<String>,
    name: String,
    parent: String,
) -> Result<Library, String> {
    crate::diagnostics::spawn_background("save_collection", move || {
        update(&app.state::<AppState>(), |l| {
            galfriends_core::collections::save(l, original.as_deref(), &name, &parent)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn delete_collections(
    app: tauri::AppHandle,
    names: Vec<String>,
) -> Result<Library, String> {
    crate::diagnostics::spawn_background("delete_collections", move || {
        update(&app.state::<AppState>(), |l| {
            galfriends_core::collections::remove(l, &names)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn open_source(source: Source, id: String) -> Result<(), String> {
    let numeric = if source == Source::Vndb {
        id.strip_prefix('c').ok_or("Invalid ID")?
    } else {
        &id
    };
    if numeric.is_empty() || !numeric.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid source ID".into());
    }
    let url = match source {
        Source::Bangumi => format!("https://bgm.tv/character/{id}"),
        Source::Kungal => format!("https://developer.nextmoe.dev/explore/character/{id}"),
        Source::Vndb => format!("https://vndb.org/{id}"),
        Source::Erogamescape => {
            format!("https://erogamescape.dyndns.org/~ap2/ero/toukei_kaiseki/game.php?game={id}")
        }
    };
    open::that_detached(url).map_err(|e| e.to_string())
}
