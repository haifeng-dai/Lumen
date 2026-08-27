use crate::app_state::data::DataStore;
use crate::ui::{
    components::{FolderSelector, TagSelector, ToastOverlay},
    views::{
        literature::{LiteratureDetailView, LiteratureListView, LiteraturePanel},
        subscription::{SubscriptionDetailView, SubscriptionListView, SubscriptionPanel},
        toolbar::ToolbarView,
    },
};
use gpui::{Entity, EventEmitter, Pixels, Point, Subscription, Window, actions, prelude::*};
use models::Literature;
use services::app::MainApp;
use std::sync::Arc;

mod actions;
mod batch;
mod layout;
mod menu;
mod menus;
mod modals;
mod new;
mod render;
mod selection;
mod toolbar;
pub(crate) mod utils;
pub use utils::render_separator;
mod types;

pub(crate) use actions::AppPdfDelegate;
pub use menu::ContextMenuType;
pub use menus::build_app_menus;
pub(crate) use types::BatchSource;
pub use types::ViewEvent;

const SIDEBAR_MIN_RATIO: f32 = 0.10;
const SIDEBAR_MAX_RATIO: f32 = 0.35;

actions!(
    main_window,
    [Cancel, ShowAbout, ShowSettings, HandleSyncConflicts]
);

impl EventEmitter<ViewEvent> for MainWindow {}
impl EventEmitter<ViewEvent> for FolderSelector {}

/// 主视图 - 整合三个面板
pub struct MainWindow {
    app: Arc<MainApp>,
    data_store: gpui::Entity<DataStore>,
    literature_panel: Entity<LiteraturePanel>,
    subscription_panel: Entity<SubscriptionPanel>,
    literature_list: Entity<LiteratureListView>,
    literature_detail: Entity<LiteratureDetailView>,
    subscription_list: Entity<SubscriptionListView>,
    subscription_detail: Entity<SubscriptionDetailView>,
    toolbar_view: Entity<ToolbarView>,
    current_window_width: Pixels,
    current_window_height: Pixels,
    /// 加载中模态框
    loading_modal: Option<String>,
    /// 全局右键菜单状态: (位置, 菜单视图)
    context_menu: Option<(Point<Pixels>, gpui::Entity<gpui_component::menu::PopupMenu>)>,
    /// 是否有活动的弹出窗口（设置、对比等）
    active_popup_count: u32,
    /// 独立 PDF 窗口控制器弱引用
    pdf_window_controller: Option<gpui::WeakEntity<super::pdf_window::PdfWindowController>>,
    /// 独立 PDF 窗口句柄
    pdf_window_handle: Option<gpui::WindowHandle<gpui_component::Root>>,
    /// 标签选择器 (Entity, Position)
    tag_selector: Option<(Entity<TagSelector>, Point<Pixels>)>,
    /// 待处理的导入队列 (用于批量 BibTeX 导入)
    pending_imports: Vec<Literature>,
    /// 待处理的对比队列 (原始文献, 新文献)
    pending_compares: Vec<(Arc<Literature>, Literature)>,
    /// 待处理的选择器队列 (候选文献列表, 选择回调)
    #[allow(clippy::type_complexity)]
    pending_selectors: Vec<(
        Vec<Arc<Literature>>,
        Box<dyn Fn(&mut Self, Literature, &mut Window, &mut Context<Self>) + Send + Sync + 'static>,
    )>,
    /// 窗口边界变化订阅（保持引用以防止被丢弃）
    #[allow(dead_code)]
    pub bounds_subscription: Option<Subscription>,
    /// 窗口关闭事件订阅（保持引用以防止被丢弃）
    #[allow(dead_code)]
    pub close_subscription: Option<Subscription>,
    /// 自身窗口句柄
    self_handle: gpui::AnyWindowHandle,
    /// 文献抓取对话框 (Dialog 内联版)
    fetch_dialog: Option<Entity<crate::ui::dialogs::FetchDialog>>,
    /// 添加/编辑订阅对话框 (Dialog 内联版)
    subscription_dialog: Option<Entity<crate::ui::dialogs::SubscriptionDialog>>,
    /// 重复文献组对话框
    duplicate_dialog: Option<Entity<crate::ui::dialogs::DuplicateListDialog>>,
    /// 已弹过 Toast 的同步错误消息（去重，避免 UiChanged 反复广播时重复弹窗）
    last_metadata_error: Option<String>,
    /// 已弹过 Toast 的附件同步错误消息（去重）
    last_attach_error: Option<String>,
    toast_overlay: Entity<ToastOverlay>,
    left_width: Pixels,
    right_width: Pixels,
}

#[derive(Clone)]
pub struct DraggedSidebar(pub components::Side);

impl Render for DraggedSidebar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}
