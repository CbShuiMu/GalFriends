//! 有界、超时的只读请求。Bangumi 相册的私有接口变化必须显式报错。
use crate::characters::{
    Alias, Artwork, Candidate, Character, Snapshot, Source, Tag, from_bangumi, text,
};
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};
pub mod tag_lookup;
mod works;

/// 角色查询客户端只向当前来源的官方 API 发送凭据；相册和图片端点不携带 Token。
pub struct CatalogClient {
    inner: Client,
    origin: &'static str,
    authorization: Option<reqwest::header::HeaderValue>,
}
impl CatalogClient {
    fn authorize(&self, request: RequestBuilder, url: &str) -> RequestBuilder {
        if url.starts_with(self.origin)
            && let Some(value) = &self.authorization
        {
            request.header(reqwest::header::AUTHORIZATION, value.clone())
        } else {
            request
        }
    }
    fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
        let url = url.as_ref();
        self.authorize(self.inner.get(url), url)
    }
    fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
        let url = url.as_ref();
        self.authorize(self.inner.post(url), url)
    }
}
/// 将本应用凭据限定到单一来源，空令牌保持公开查询。
pub fn catalog_client(proxy: &str, source: Source, token: &str) -> Result<CatalogClient, String> {
    use crate::connections::{Service, client::authorization};
    let (origin, service) = match source {
        Source::Bangumi => ("https://api.bgm.tv/", Some(Service::Bangumi)),
        Source::Kungal => ("https://api.nextmoe.dev/", Some(Service::Kungal)),
        Source::Vndb => ("https://api.vndb.org/", Some(Service::Vndb)),
        Source::Erogamescape => ("", None),
    };
    let authorization = if token.is_empty() {
        None
    } else {
        Some(
            authorization(service.ok_or("Public source has no credentials")?, token)
                .map_err(|e| e.to_string())?,
        )
    };
    Ok(CatalogClient {
        inner: client_builder(proxy)?
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?,
        origin,
        authorization,
    })
}

/// 使用系统代理，或用户显式配置的 HTTP/SOCKS 代理。
pub fn client(proxy: &str) -> Result<Client, String> {
    client_builder(proxy)?.build().map_err(|e| e.to_string())
}
pub(crate) fn client_builder(proxy: &str) -> Result<reqwest::blocking::ClientBuilder, String> {
    let mut b = Client::builder()
        .user_agent("GalFriends/0.1 (local character library)")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(35));
    if !proxy.trim().is_empty() {
        let url = reqwest::Url::parse(proxy).map_err(|_| "Invalid proxy URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Use an HTTP proxy without embedded credentials".into());
        }
        b = b.proxy(reqwest::Proxy::all(proxy).map_err(|e| e.to_string())?);
    }
    Ok(b)
}

fn response(r: RequestBuilder) -> Result<Vec<u8>, String> {
    let r = r
        .send()
        .map_err(|e| e.without_url().to_string())?
        .error_for_status()
        .map_err(|e| e.without_url().to_string())?;
    let mut bytes = Vec::new();
    r.take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Response too large".into());
    }
    Ok(bytes)
}
fn request(r: RequestBuilder) -> Result<Value, String> {
    serde_json::from_slice(&response(r)?).map_err(|e| e.to_string())
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    v[key]
        .as_array()
        .ok_or_else(|| format!("Invalid API response: {key}"))
}
const VN_FIELDS: &str = "name,original,aliases,description,blood_type,height,weight,bust,waist,hips,cup,age,birthday,sex,gender,vns{id,title,alttitle,role,spoiler},traits{id,name,group_name,spoiler,sexual,lie}";
fn identifier(source: Source, id: &str) -> Result<(), String> {
    let digits = if source == Source::Vndb {
        id.strip_prefix('c').ok_or("Invalid VNDB ID")?
    } else {
        id
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid source ID".into());
    }
    Ok(())
}
/// 搜索最多 20 项候选；保持每个源的独立身份。
pub fn search(c: &CatalogClient, source: Source, q: &str) -> Result<Vec<Candidate>, String> {
    let q = q.trim();
    if q.is_empty() || q.chars().count() > 200 {
        return Err("Enter 1–200 characters".into());
    }
    let (v, key) = match source {
        Source::Bangumi => (
            request(
                c.post("https://api.bgm.tv/v0/search/characters?limit=20")
                    .json(&json!({"keyword":q})),
            )?,
            "data",
        ),
        Source::Kungal => (
            request(
                c.get("https://api.nextmoe.dev/v2/catalog/characters")
                    .query(&[
                        ("q", q),
                        ("limit", "20"),
                        ("view", "full"),
                        ("nsfw", "true"),
                    ]),
            )?,
            "items",
        ),
        Source::Vndb => (
            request(c.post("https://api.vndb.org/kana/character").json(
                &json!({"filters":["search","=",q],"fields":"name,original,aliases","results":20}),
            ))?,
            "results",
        ),
        Source::Erogamescape => return erogamescape(&c.inner, q),
    };
    array(&v, key)?
        .iter()
        .map(|v| {
            let id = v["id"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v["id"].to_string());
            identifier(source, &id)?;
            let name = if source == Source::Kungal {
                text(v, "display_name")
            } else if source == Source::Vndb && !text(v, "original").is_empty() {
                text(v, "original")
            } else {
                text(v, "name")
            };
            Ok(Candidate {
                source,
                id,
                romanized_name: search_romanized_name(source, v, &name),
                name,
                subtitle: if source == Source::Vndb {
                    text(v, "name")
                } else {
                    text(v, "summary")
                },
                portrait: search_portrait(source, v),
            })
        })
        .collect()
}

fn search_romanized_name(source: Source, value: &Value, name: &str) -> Option<String> {
    let romanized = match source {
        Source::Bangumi => value["infobox"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["key"] == "别名")
            .flat_map(|row| row["value"].as_array().into_iter().flatten())
            .filter(|alias| alias["k"] == "罗马字")
            .filter_map(|alias| alias["v"].as_str())
            .find(|name| !name.trim().is_empty()),
        Source::Kungal => value["latin"].as_str(),
        Source::Vndb => value["name"].as_str(),
        Source::Erogamescape => None,
    };
    romanized
        .map(str::trim)
        .filter(|romanized| !romanized.is_empty() && *romanized != name.trim())
        .map(str::to_owned)
}

fn search_portrait(source: Source, value: &Value) -> Option<String> {
    if source != Source::Bangumi {
        return None;
    }
    ["medium", "large", "small", "grid"]
        .into_iter()
        .filter_map(|size| value["images"][size].as_str())
        .find(|url| bangumi_image(url))
        .map(str::to_owned)
}

fn bangumi_artworks(snapshots: &[Snapshot], photos: &[Value]) -> Vec<Artwork> {
    let mut artworks: Vec<_> = photos
        .iter()
        .filter(|photo| !text(photo, "title").contains("待删除"))
        .filter_map(|photo| {
            let url = photo["images"]["large"].as_str()?;
            bangumi_image(url).then(|| Artwork {
                id: photo["id"].to_string(),
                title: text(photo, "title"),
                url: url.into(),
                original: url.into(),
                spoiler: photo["spoiler"].as_bool().unwrap_or_default(),
                ..Default::default()
            })
        })
        .collect();
    if let Some(portrait) = missing_bangumi_portrait(snapshots, &artworks) {
        artworks.insert(0, portrait);
    }
    artworks
}

/// 从 Bangumi 详情快照提取尚未入库的主立绘，供首次导入和旧库补图共用。
/// 相册已有同一图片时保留相册条目的标题和剧透标记。
pub fn missing_bangumi_portrait(snapshots: &[Snapshot], artworks: &[Artwork]) -> Option<Artwork> {
    let snapshot = snapshots.iter().find(|s| s.source == Source::Bangumi)?;
    let detail = snapshot.data.get("character").unwrap_or(&snapshot.data);
    let url = detail["images"]["large"].as_str()?;
    let location = reqwest::Url::parse(url).ok()?;
    if !bangumi_image(url)
        || location.path() == "/img/no_icon_subject.png"
        || artworks.iter().any(|art| {
            art.id == "portrait"
                || (bangumi_image(&art.original)
                    && reqwest::Url::parse(&art.original)
                        .is_ok_and(|original| original.path() == location.path()))
        })
    {
        return None;
    }
    Some(Artwork {
        id: "portrait".into(),
        title: text(detail, "name"),
        url: url.into(),
        original: url.into(),
        ..Default::default()
    })
}

/// 获取详情及关联数据；任何一部分失败都不保存不完整的成功记录。
pub fn fetch(c: &CatalogClient, source: Source, id: &str) -> Result<Character, String> {
    identifier(source, id)?;
    match source {
        Source::Bangumi => {
            let v = request(c.get(format!("https://api.bgm.tv/v0/characters/{id}")))?;
            let mut ch = from_bangumi(v.clone())?;
            let subjects =
                request(c.get(format!("https://api.bgm.tv/v0/characters/{id}/subjects")))?;
            let persons = request(c.get(format!("https://api.bgm.tv/v0/characters/{id}/persons")))?;
            let mut photos = Vec::new();
            let mut offset = 0;
            loop {
                let page = request(
                    c.get(format!("https://next.bgm.tv/p1/characters/{id}/photos"))
                        .query(&[("limit", 100), ("offset", offset)]),
                )?;
                let rows = array(&page, "data")?;
                photos.extend(rows.iter().cloned());
                if rows.len() < 100 {
                    break;
                }
                offset += 100;
                if offset >= 2000 {
                    return Err("Album exceeds safety limit".into());
                }
            }
            ch.artworks = bangumi_artworks(&ch.snapshots, &photos);
            ch.snapshots[0].data =
                json!({"character":v,"subjects":subjects,"persons":persons,"album":photos});
            Ok(ch)
        }
        Source::Vndb => {
            let v = request(
                c.post("https://api.vndb.org/kana/character")
                    .json(&json!({"filters":["id","=",id],"fields":VN_FIELDS})),
            )?;
            let item = array(&v, "results")?.first().ok_or("Character not found")?;
            from_vndb(item.clone())
        }
        Source::Kungal => {
            let v = request(
                c.get(format!(
                    "https://api.nextmoe.dev/v2/catalog/characters/{id}"
                ))
                .query(&[
                    ("view", "full"),
                    ("nsfw", "true"),
                    ("spoiler", "major"),
                ]),
            )?;
            let mut appearances = Vec::new();
            let mut cursor = String::new();
            loop {
                let mut req = c
                    .get(format!(
                        "https://api.nextmoe.dev/v2/catalog/characters/{id}/appearances"
                    ))
                    .query(&[("limit", "100"), ("nsfw", "true")]);
                if !cursor.is_empty() {
                    req = req.query(&[("cursor", &cursor)]);
                }
                let page = request(req)?;
                appearances.extend(array(&page, "items")?.iter().cloned());
                let next = page["next_cursor"].as_str().unwrap_or_default();
                if next.is_empty() {
                    break;
                }
                if next == cursor || appearances.len() > 2000 {
                    return Err("Invalid appearances cursor".into());
                }
                cursor = next.to_owned();
            }
            let mut ch = from_kungal(v)?;
            let work_details = works::details(c, &appearances)?;
            ch.snapshots[0].data = json!({"character":ch.snapshots[0].data,"appearances":appearances,
                    "work_details":work_details});
            Ok(ch)
        }
        Source::Erogamescape => Err("Erogamescape provides works, not characters".into()),
    }
}
/// 唯一允许的远程立绘域。
pub fn bangumi_image(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https" && u.host_str() == Some("lain.bgm.tv") && u.username().is_empty()
    })
}
/// 保留鲲 Gal 的完整角色对象；不使用其图像作为立绘。
pub fn from_kungal(v: Value) -> Result<Character, String> {
    let id = text(&v, "id");
    identifier(Source::Kungal, &id)?;
    let mut aliases = Vec::new();
    if let Some(l) = v["localized"].as_object() {
        for (kind, a) in l {
            aliases.push(Alias {
                kind: kind.clone(),
                name: text(a, "value"),
            });
        }
    }
    for a in v["aliases"].as_array().into_iter().flatten() {
        aliases.push(Alias {
            kind: text(a, "lang"),
            name: a
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| text(a, "value")),
        });
    }
    let tags = v["traits"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| Tag {
            name: t["localized"]["zh-Hans"]["value"]
                .as_str()
                .unwrap_or_else(|| t["display_name"].as_str().unwrap_or_default())
                .into(),
            group: text(t, "group"),
            source: Source::Kungal,
            spoiler: t["spoiler"] != "none",
            sexual: t["is_sexual"].as_bool().unwrap_or(false),
        })
        .collect();
    let summary = v["intros"]
        .as_array()
        .and_then(|a| a.first())
        .map(|a| text(a, "value"))
        .unwrap_or_default();
    Ok(Character {
        id: format!("kungal:{id}"),
        name: text(&v, "display_name"),
        summary,
        aliases,
        tags,
        snapshots: vec![Snapshot {
            source: Source::Kungal,
            id: id.clone(),
            url: format!("https://developer.nextmoe.dev/explore/character/{id}"),
            data: v,
        }],
        ..Default::default()
    })
}
/// VNDB traits 与 Bangumi 立绘独立，所有别名与未展示字段保存在快照。
pub fn from_vndb(v: Value) -> Result<Character, String> {
    let id = text(&v, "id");
    identifier(Source::Vndb, &id)?;
    let mut aliases = vec![
        Alias {
            kind: "original".into(),
            name: text(&v, "original"),
        },
        Alias {
            kind: "romanized".into(),
            name: text(&v, "name"),
        },
    ];
    aliases.extend(
        v["aliases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a.as_str())
            .map(|s| Alias {
                kind: "alias".into(),
                name: s.into(),
            }),
    );
    let tags = v["traits"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| Tag {
            name: text(t, "name"),
            group: text(t, "group_name"),
            source: Source::Vndb,
            spoiler: t["spoiler"].as_u64().unwrap_or(0) > 0,
            sexual: t["sexual"].as_bool().unwrap_or(false),
        })
        .collect();
    let original = text(&v, "original");
    Ok(Character {
        id: format!("vndb:{id}"),
        name: if original.is_empty() {
            text(&v, "name")
        } else {
            original
        },
        summary: text(&v, "description"),
        aliases,
        tags,
        snapshots: vec![Snapshot {
            source: Source::Vndb,
            id: id.clone(),
            url: format!("https://vndb.org/{id}"),
            data: v,
        }],
        ..Default::default()
    })
}
fn erogamescape(c: &Client, q: &str) -> Result<Vec<Candidate>, String> {
    // 复用 GalGear 的十六进制字面量规则，通配符也先转义。
    let q = q.replace('!', "!!").replace('%', "!%").replace('_', "!_");
    let hex: String = q.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let sql = format!(
        "SELECT id, gamename FROM gamelist WHERE gamename ILIKE '%' || convert_from(decode('{hex}','hex'),'UTF8') || '%' ESCAPE '!' ORDER BY id LIMIT 20"
    );
    let bytes = response(
        c.post("https://erogamescape.dyndns.org/~ap2/ero/toukei_kaiseki/sql_for_erogamer_form.php")
            .form(&[("sql", sql)]),
    )?;
    let html = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let doc = scraper::Html::parse_document(html);
    let table = scraper::Selector::parse("table").map_err(|e| e.to_string())?;
    let row = scraper::Selector::parse("tr").map_err(|e| e.to_string())?;
    let cell = scraper::Selector::parse("th,td").map_err(|e| e.to_string())?;
    for t in doc.select(&table) {
        let mut rows = t.select(&row);
        let header = rows
            .next()
            .map(|r| {
                r.select(&cell)
                    .map(|c| c.text().collect::<String>().trim().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if header != ["id", "gamename"] {
            continue;
        }
        return rows
            .map(|r| {
                let v: Vec<_> = r
                    .select(&cell)
                    .map(|c| c.text().collect::<String>().trim().to_owned())
                    .collect();
                if v.len() != 2 {
                    return Err("Invalid Erogamescape row".into());
                }
                identifier(Source::Erogamescape, &v[0])?;
                Ok(Candidate {
                    source: Source::Erogamescape,
                    id: v[0].clone(),
                    name: v[1].clone(),
                    subtitle: String::new(),
                    romanized_name: None,
                    portrait: None,
                })
            })
            .collect();
    }
    Err("Erogamescape returned no result table".into())
}

#[cfg(test)]
mod tests {
    use super::{
        bangumi_artworks, bangumi_image, from_kungal, from_vndb, missing_bangumi_portrait,
    };
    use crate::characters::{Snapshot, Source};
    use serde_json::{Value, json};

    #[test]
    fn bangumi_portrait_is_available_alongside_album_and_from_legacy_snapshot() {
        let url = "https://lain.bgm.tv/pic/crt/l/b1/14/76897_crt_23HE8.jpg?r=1754678449";
        let detail = json!({"name":"神堂悠果", "images":{"large":url}});
        let photos = [json!({"id":21289,"title":"私服","images":{
            "large":"https://lain.bgm.tv/pic/photos/character/l/d3/0b/76897_cnCcf.jpg"
        }})];
        let mut snapshot = Snapshot {
            source: Source::Bangumi,
            id: "76897".into(),
            url: "https://bgm.tv/character/76897".into(),
            data: json!({"character":detail,"album":photos}),
        };
        let artworks = bangumi_artworks(std::slice::from_ref(&snapshot), &photos);
        assert_eq!(artworks.len(), 2);
        assert_eq!(artworks[0].original, url);
        assert_eq!(artworks[1].id, "21289");

        snapshot.data = detail;
        assert_eq!(
            missing_bangumi_portrait(std::slice::from_ref(&snapshot), &artworks[1..]).as_ref(),
            Some(&artworks[0])
        );
        assert!(missing_bangumi_portrait(&[snapshot], &artworks).is_none());
    }

    #[test]
    fn bangumi_portrait_skips_album_duplicate_and_keeps_its_spoiler_flag() {
        let album_url = "https://lain.bgm.tv/pic/crt/l/portrait.jpg";
        let mut snapshot = Snapshot {
            source: Source::Bangumi,
            id: "1".into(),
            url: String::new(),
            data: json!({"images":{"large": format!("{album_url}?r=123")}}),
        };
        let photos = [json!({"id":1,"title":"差分","spoiler":true,
            "images":{"large":album_url}
        })];
        let artworks = bangumi_artworks(std::slice::from_ref(&snapshot), &photos);
        assert_eq!(artworks.len(), 1);
        assert!(artworks[0].spoiler);

        for url in [
            "https://lain.bgm.tv/img/no_icon_subject.png",
            "https://example.org/pic/crt/l/portrait.jpg",
        ] {
            snapshot.data["images"]["large"] = json!(url);
            assert!(missing_bangumi_portrait(std::slice::from_ref(&snapshot), &[]).is_none());
        }
    }
    #[test]
    fn romanized_search_names_follow_each_source_without_guessing_missing_readings() {
        use crate::characters::Source;
        let bangumi: Value =
            serde_json::from_str(include_str!("../../../fixtures/setsuna-bangumi.json")).unwrap();
        assert_eq!(
            super::search_romanized_name(Source::Bangumi, &bangumi, "神堂刹那").as_deref(),
            Some("Shindou Setsuna")
        );
        let vndb: Value =
            serde_json::from_str(include_str!("../../../fixtures/setsuna-vndb.json")).unwrap();
        assert_eq!(
            super::search_romanized_name(Source::Vndb, &vndb["results"][0], "神堂 刹那").as_deref(),
            Some("Shindou Setsuna")
        );
        let kungal = serde_json::json!({"latin":"  Shindou Setsuna  ","display_name":"神堂刹那"});
        assert_eq!(
            super::search_romanized_name(Source::Kungal, &kungal, "神堂刹那").as_deref(),
            Some("Shindou Setsuna")
        );

        for value in [
            Value::Null,
            serde_json::json!({"latin":null,"name":null}),
            serde_json::json!({"latin":"  ","name":"  "}),
            serde_json::json!({"latin":" Shindou Setsuna ","name":"Shindou Setsuna"}),
        ] {
            for source in [
                Source::Bangumi,
                Source::Kungal,
                Source::Vndb,
                Source::Erogamescape,
            ] {
                assert!(super::search_romanized_name(source, &value, "Shindou Setsuna").is_none());
            }
        }
        let kana_only = serde_json::json!({"infobox":[{"key":"别名","value":[{"k":"纯假名","v":"しんどう せつな"}]}]});
        assert!(super::search_romanized_name(Source::Bangumi, &kana_only, "神堂刹那").is_none());
        let alias_only = serde_json::json!({"aliases":[{"lang":"ja","value":"Shindou Setsuna","alias_kind":"spelling_variant"}]});
        assert!(super::search_romanized_name(Source::Kungal, &alias_only, "神堂刹那").is_none());
    }

    #[test]
    fn search_portraits_use_available_bangumi_images_and_reject_external_urls() {
        use crate::characters::Source;
        let mut value = serde_json::json!({"images": {
            "medium": "https://lain.bgm.tv/r/400/pic/crt/l/portrait.jpg",
            "large": "https://lain.bgm.tv/pic/crt/l/portrait.jpg"
        }});
        assert_eq!(
            super::search_portrait(Source::Bangumi, &value).as_deref(),
            value["images"]["medium"].as_str()
        );
        value["images"]["medium"] = serde_json::json!("https://lain.bgm.tv.evil.test/portrait.jpg");
        assert_eq!(
            super::search_portrait(Source::Bangumi, &value).as_deref(),
            value["images"]["large"].as_str()
        );
        for source in [Source::Kungal, Source::Vndb, Source::Erogamescape] {
            assert!(super::search_portrait(source, &value).is_none());
        }
        value["images"]["large"] = Value::Null;
        assert!(super::search_portrait(Source::Bangumi, &value).is_none());
        assert!(super::search_portrait(Source::Bangumi, &Value::Null).is_none());
    }
    #[test]
    fn catalog_credentials_are_restricted_to_the_selected_api_origin() {
        let client =
            super::catalog_client("", crate::characters::Source::Bangumi, "fixture-key").unwrap();
        let request = client.get("https://api.bgm.tv/v0/me").build().unwrap();
        assert_eq!(request.headers()["authorization"], "Bearer fixture-key");
        assert!(request.headers()["authorization"].is_sensitive());
        for url in [
            "https://next.bgm.tv/p1/characters/1/photos",
            "https://lain.bgm.tv/image.png",
            "https://api.bgm.tv.evil.test/",
            "https://api.vndb.org/kana/authinfo",
        ] {
            assert!(
                !client
                    .get(url)
                    .build()
                    .unwrap()
                    .headers()
                    .contains_key("authorization")
            );
        }
    }
    #[test]
    fn kungal_preserves_names_without_using_its_portrait() {
        let v: Value =
            serde_json::from_str(include_str!("../../../fixtures/setsuna-kungal-search.json"))
                .unwrap();
        let c = from_kungal(v["items"][0].clone()).unwrap();
        assert!(c.artworks.is_empty());
        assert!(c.aliases.iter().any(|a| a.name == "Shindou Setsuna"));
        assert!(!c.tags.is_empty());
        assert_eq!(c.snapshots[0].data, v["items"][0]);
    }
    #[test]
    fn vndb_preserves_unknown_and_spoiler_fields() {
        let v: Value =
            serde_json::from_str(include_str!("../../../fixtures/setsuna-vndb.json")).unwrap();
        let c = from_vndb(v["results"][0].clone()).unwrap();
        assert_eq!(c.snapshots[0].data, v["results"][0]);
        assert!(c.artworks.is_empty());
    }
    #[test]
    fn image_urls_cannot_cross_source_boundaries() {
        assert!(bangumi_image("https://lain.bgm.tv/pic/a.jpg"));
        for bad in [
            "http://lain.bgm.tv/a.jpg",
            "https://lain.bgm.tv.evil.test/x",
            "file:///C:/private",
            "https://evil.test/lain.bgm.tv",
            "https://user@lain.bgm.tv/a",
        ] {
            assert!(!bangumi_image(bad));
        }
    }
}
