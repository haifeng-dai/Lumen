use super::{FetchSource, MainWindow};

use super::types::BatchSource;
use crate::ui::notification::show_notification;
use components::IconName;
use gpui::anchored;
use gpui::prelude::*;
use gpui::{App, Hsla, Pixels, Point, Window, px};
use gpui::{ClipboardItem, SharedString, div};
use gpui_component::notification::NotificationType;
use gpui_component::{
    ActiveTheme, Icon,
    menu::{PopupMenu, PopupMenuItem},
};
use i18n::{I18nKey, Language, t};
use models::{Folder, FolderType, Literature};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parser::export::ExportFormat;
use services::app::MainApp;

mod attachment;
mod folder;
mod literature;
mod subscription;
mod tag;

/// 文件夹名称映射（id -> 显示名）
type FolderNameMap = Arc<HashMap<String, String>>;
/// 文件夹子级映射（父 id -> 子 id 列表）
type FolderChildrenMap = Arc<HashMap<Option<String>, Vec<String>>>;
/// 文件夹选择回调
type FolderSelectClosure = Arc<dyn Fn(&str, &mut Window, &mut App) + 'static>;
/// Literature 菜单预取数据
type LiteraturePrefetch = (
    usize,
    HashSet<String>,
    bool,
    Option<Literature>,
    (FolderNameMap, FolderChildrenMap),
);

/// 构造"破坏性操作"右键菜单项：红色文本 + 红色图标。
///
/// `PopupMenuItem` 本身不暴露文本颜色方法，故用 `element()` 自绘内容，
/// 并通过 `.icon()` 传入已着色的红色 `Icon` 复用默认左图标槽位（保证对齐）。
/// 返回的是普通 `PopupMenuItem`，调用处照常链式 `.on_click(...)` 即可。
fn danger_menu_item(danger: Hsla, label: impl Into<SharedString>, icon: IconName) -> PopupMenuItem {
    let label = label.into();
    PopupMenuItem::element(move |_window, cx| {
        div().text_color(cx.theme().danger).child(label.clone())
    })
    .icon(Icon::new(icon).text_color(danger))
}

/// 从文件夹列表构建层级映射：`name_map`(id->显示名) 与 `children_map`(父id->子id列表)。
/// 仅纳入用户自定义文件夹（`FolderType::Custom`）。
fn build_folder_maps(folders: &[Arc<Folder>]) -> (FolderNameMap, FolderChildrenMap) {
    let mut name_map: HashMap<String, String> = HashMap::new();
    let mut children_map: HashMap<Option<String>, Vec<String>> = HashMap::new();
    for f in folders {
        if f.folder_type != FolderType::Custom {
            continue;
        }
        name_map.insert(f.id.clone(), f.name.clone());
        children_map
            .entry(f.parent_id.clone())
            .or_default()
            .push(f.id.clone());
    }
    // 子文件夹按名称排序，保证菜单展示稳定
    for children in children_map.values_mut() {
        children.sort_by(|a, b| {
            name_map
                .get(a)
                .map(|s| s.to_lowercase())
                .cmp(&name_map.get(b).map(|s| s.to_lowercase()))
        });
    }
    (Arc::new(name_map), Arc::new(children_map))
}

/// 递归把文件夹树渲染为嵌套 `PopupMenu`：
/// - 叶子文件夹 → 直接可点击项（点击 = 加入该文件夹）
/// - 有子文件夹的父文件夹 → `submenu`，其首项为文件夹本名（点击 = 加入该文件夹），下挂子文件夹
///
/// 层级不限，hover 实时展开。父文件夹“既能加入又能展开”通过“子菜单首项用本名”实现，
/// 避免方案 B 的“添加到『X』”丑前缀，也无需自定义菜单组件。
fn build_folder_level(
    mut m: PopupMenu,
    parent_id: Option<String>,
    name_map: &FolderNameMap,
    children_map: &FolderChildrenMap,
    on_select: &FolderSelectClosure,
    window: &mut Window,
    cx: &mut gpui::Context<PopupMenu>,
) -> PopupMenu {
    if let Some(children) = children_map.get(&parent_id) {
        for child_id in children {
            let name = name_map.get(child_id).cloned().unwrap_or_default();
            let has_children = children_map
                .get(&Some(child_id.clone()))
                .map(|c| !c.is_empty())
                .unwrap_or(false);
            if has_children {
                let child_name = name.clone();
                let name_map_c = name_map.clone();
                let children_map_c = children_map.clone();
                let on_select_c = on_select.clone();
                let child_id_c = child_id.clone();
                m = m.submenu(child_name, window, cx, move |mut sub, window, cx| {
                    // 首项：加入本文件夹（用文件夹本名，无丑前缀）
                    let fid2 = child_id_c.clone();
                    let on_select2 = on_select_c.clone();
                    sub = sub.item(PopupMenuItem::new(name.clone()).on_click(
                        move |_, window, cx| {
                            on_select2(&fid2, window, cx);
                        },
                    ));
                    // 递归子文件夹
                    sub = build_folder_level(
                        sub,
                        Some(child_id_c.clone()),
                        &name_map_c,
                        &children_map_c,
                        &on_select_c,
                        window,
                        cx,
                    );
                    sub
                });
            } else {
                let fid = child_id.clone();
                let on_select_self = on_select.clone();
                let self_item = PopupMenuItem::new(name).on_click(move |_, window, cx| {
                    on_select_self(&fid, window, cx);
                });
                m = m.item(self_item);
            }
        }
    }
    m
}

/// 按指定格式（BibTeX / IEEE / 爱思微尔）生成选中文献的引用文本，
/// 复制到剪切板并弹「已复制到剪切板」通知。空选择 / 生成失败分别给错误通知。
fn copy_citation(
    app: &Arc<MainApp>,
    ids: &HashSet<String>,
    format: ExportFormat,
    lang: Language,
    cx: &mut App,
) {
    let lits: Vec<Literature> = app
        .db
        .get_all_literatures()
        .unwrap_or_default()
        .into_iter()
        .filter(|l| ids.contains(&l.id))
        .collect();
    let abbreviate_journal = app.config.lock().unwrap().citation.abbreviate_journal;
    match app
        .export_manager
        .export_to_string(format, &lits, abbreviate_journal)
    {
        Ok(text) if !text.trim().is_empty() => {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            show_notification(
                NotificationType::Success,
                t(I18nKey::CopiedToClipboard, lang),
                cx,
            );
        }
        Ok(_) => show_notification(
            NotificationType::Error,
            t(I18nKey::NoLiteratureSelectedForCitation, lang),
            cx,
        ),
        Err(e) => show_notification(
            NotificationType::Error,
            format!("{}: {e}", t(I18nKey::CitationError, lang)),
            cx,
        ),
    }
}

/// 右键菜单类型
#[derive(Clone, Debug)]
pub enum ContextMenuType {
    Folder(Option<String>),       // 文件夹ID，None表示空白处
    Tag(Option<String>),          // 标签ID，None表示空白处
    Subscription(Option<String>), // 订阅ID，None表示空白处
    SubscriptionAll,              // 「所有订阅」行右键（更新所有订阅）
    SubscriptionItem(String),     // 订阅条目ID
    Literature(String),           // 文献ID
    Attachment(String),           // 附件ID
}

impl MainWindow {
    /// 关闭所有活动的右键菜单和子菜单
    pub fn close_menus(&mut self, cx: &mut Context<Self>) {
        self.context_menu = None;
        cx.notify();
    }

    pub fn show_context_menu(
        &mut self,
        pos: Point<Pixels>,
        menu_type: ContextMenuType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu_view = self.build_context_menu(menu_type, window, cx);
        self.context_menu = Some((pos, menu_view));
        cx.notify();
    }

    pub(super) fn render_global_context_menu(
        &self,
        _cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let (pos, menu_view) = self.context_menu.as_ref()?;
        let mut pos = *pos;
        let menu_width = px(160.0);

        // 边界检测：防止菜单超出右侧
        if pos.x + menu_width > self.current_window_width {
            pos.x -= menu_width;
        }

        // 使用 gpui 的 anchored() 直接锚定原生 PopupMenu 视图实体
        let element = anchored().position(pos).child(menu_view.clone());

        Some(element)
    }

    /// 构建原生 PopupMenu 实体与级联二级菜单
    pub fn build_context_menu(
        &self,
        menu_type: ContextMenuType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<PopupMenu> {
        let lang = self.app.current_language();
        let this_weak = cx.weak_entity();

        // ── 提前在 PopupMenu::build 之前读取所有需要 cx/self 的数据 ──
        // 这样闭包内就不需要再借用 cx，彻底避免二次借用 panic

        let current_selected_folder = cx
            .global::<crate::app_state::ui::UiState>()
            .selected_folder_id
            .clone();

        // SubscriptionItem 分支所需数据
        let sub_item_state: Option<(bool, bool)> =
            if let ContextMenuType::SubscriptionItem(ref sub_id) = menu_type {
                let data = self.data_store.read(cx);
                Some(
                    if let Some(s) = data.feed_items.iter().find(|s| &s.id == sub_id) {
                        (s.is_read, s.is_added_to_library)
                    } else {
                        (false, false)
                    },
                )
            } else {
                None
            };

        // SubscriptionItem 分支：预取自定义文件夹层级映射（用于「添加到文献库」多级子菜单）
        let (sub_name_map, sub_children_map): (FolderNameMap, FolderChildrenMap) =
            if let ContextMenuType::SubscriptionItem(_) = menu_type {
                let data = self.data_store.read(cx);
                build_folder_maps(&data.folders)
            } else {
                (Arc::new(HashMap::new()), Arc::new(HashMap::new()))
            };

        // Attachment 分支所需数据
        let attachment_lit_data: Option<(String, bool, String)> =
            if let ContextMenuType::Attachment(ref att_id) = menu_type {
                let data = self.data_store.read(cx);
                data.literatures.iter().find_map(|l| {
                    l.attachments
                        .iter()
                        .find(|a| &a.id == att_id)
                        .map(|a| (l.id.clone(), a.is_main, a.file_path.clone()))
                })
            } else {
                None
            };

        // Literature 分支所需数据
        let literature_prefetch: Option<LiteraturePrefetch> =
            if let ContextMenuType::Literature(ref lit_id) = menu_type {
                let ui = cx.global::<crate::app_state::ui::UiState>();
                let selected_count = ui.selected_literature_ids.len();
                let selected_ids = ui.selected_literature_ids.clone();
                let _ = ui; // 释放不可变借用

                let data = self.data_store.read(cx);
                let lit = data
                    .literatures
                    .iter()
                    .find(|l| l.id == *lit_id)
                    .map(|l| (**l).clone());
                let in_trash = lit
                    .as_ref()
                    .is_some_and(|l| l.folder_ids.contains(&"trash".to_string()));

                // 自定义文件夹层级映射
                let (custom_name_map, custom_children_map) = build_folder_maps(&data.folders);

                Some((
                    selected_count,
                    selected_ids,
                    in_trash,
                    lit,
                    (custom_name_map, custom_children_map),
                ))
            } else {
                None
            };

        let window_ref: &mut Window = window;
        let app_ref: &mut App = cx;

        // 统一在外部进行 PopupMenu::build 构造，使用显式的 Window 和 App 引用
        PopupMenu::build(
            window_ref,
            app_ref,
            move |menu, window, cx| match menu_type {
                ContextMenuType::Folder(target_id) => {
                    folder::build_folder_menu(menu, target_id, this_weak.clone(), lang, cx)
                }
                ContextMenuType::Tag(target_id) => {
                    tag::build_tag_menu(menu, target_id, this_weak.clone(), lang, cx)
                }
                ContextMenuType::Subscription(target_id) => subscription::build_subscription_menu(
                    menu,
                    target_id,
                    this_weak.clone(),
                    lang,
                    cx,
                ),
                ContextMenuType::SubscriptionAll => {
                    subscription::build_subscription_all_menu(menu, this_weak.clone(), lang, cx)
                }
                ContextMenuType::SubscriptionItem(sub_id) => {
                    subscription::build_subscription_item_menu(
                        menu,
                        sub_id,
                        sub_item_state,
                        sub_name_map,
                        sub_children_map,
                        window,
                        this_weak.clone(),
                        lang,
                        cx,
                    )
                }
                ContextMenuType::Attachment(att_id) => attachment::build_attachment_menu(
                    menu,
                    att_id,
                    attachment_lit_data,
                    this_weak.clone(),
                    lang,
                    cx,
                ),
                ContextMenuType::Literature(lit_id) => literature::build_literature_menu(
                    menu,
                    lit_id,
                    literature_prefetch,
                    current_selected_folder,
                    window,
                    this_weak.clone(),
                    lang,
                    cx,
                ),
            },
        )
    }
}
