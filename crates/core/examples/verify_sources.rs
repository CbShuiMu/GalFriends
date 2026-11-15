//! 显式运行的在线烟雾验证，不参与普通离线测试。
use galfriends_core::{characters::Source, sources};
fn main() -> Result<(), String> {
    let proxy = std::env::args().nth(1).unwrap_or_default();
    for (source, query) in [
        (Source::Bangumi, "神堂刹那"),
        (Source::Kungal, "神堂刹那"),
        (Source::Vndb, "Shindou Setsuna"),
    ] {
        let client = sources::catalog_client(&proxy, source, "")?;
        let found = sources::search(&client, source, query)?;
        println!("{} search: {} results", source.key(), found.len());
        let id = &found.first().ok_or("No expected sample result")?.id;
        let c = sources::fetch(&client, source, id)?;
        println!(
            "{} detail: {} aliases, {} artworks, {} tags, {} snapshots",
            source.key(),
            c.aliases.len(),
            c.artworks.len(),
            c.tags.len(),
            c.snapshots.len()
        );
    }
    let client = sources::catalog_client(&proxy, Source::Erogamescape, "")?;
    match sources::search(&client, Source::Erogamescape, "サクラノ詩") {
        Ok(v) => println!("erogamescape search: {} works", v.len()),
        Err(e) => return Err(format!("Erogamescape: {e}")),
    }
    Ok(())
}
