#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchSource {
    ArXiv,
    Doi,
    Dblp,
    OpenAlex,
}

/// 视图事件
pub enum ViewEvent {
    /// 关闭所有菜单
    CloseMenu,
}
