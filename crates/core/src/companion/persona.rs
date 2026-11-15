//! 与角色库身份显式关联的剧情和扮演资料。

/// 根据角色库 ID 选择已核对的人格资料。显示名相同的其他角色不共用资料。
pub(crate) fn for_character(id: &str) -> Option<&'static str> {
    match id {
        "bangumi:76896" => Some(include_str!("personas/setsuna.md")),
        "bangumi:76897" => Some(include_str!("personas/yuuka.md")),
        "bangumi:88852" => Some(include_str!("personas/rei.md")),
        _ => None,
    }
}
