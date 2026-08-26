use std::sync::Arc;

use gpui::{App, ParentElement, Pixels, SharedString, Styled, Window, rems};
use gpui_component::{
    button::Button,
    label::Label,
    menu::{DropdownMenu, PopupMenuItem},
};

/// 轻量级选项下拉组件（Button + DropdownMenu）。
///
/// `options`: `(value, label)` 列表
/// `current_value`: 当前选中的 value
/// `scrollable`: 下拉菜单是否可滚动
/// `on_change`: 选中项变化时的回调
pub fn selector(
    id: &'static str,
    options: Vec<(SharedString, SharedString)>,
    current_value: SharedString,
    scrollable: bool,
    on_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> impl gpui::IntoElement {
    selector_impl(
        id,
        options,
        current_value,
        scrollable,
        SelectorLayout::Default,
        on_change,
    )
}

/// 紧凑的固定宽度选择器，适合侧栏工具栏等空间受限的场景。
pub fn selector_fixed(
    id: &'static str,
    options: Vec<(SharedString, SharedString)>,
    current_value: SharedString,
    scrollable: bool,
    width: Pixels,
    on_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> impl gpui::IntoElement {
    selector_impl(
        id,
        options,
        current_value,
        scrollable,
        SelectorLayout::Fixed(width),
        on_change,
    )
}

/// 填充父容器剩余空间的选择器，适合聊天输入栏中的模型选择。
pub fn selector_fill(
    id: &'static str,
    options: Vec<(SharedString, SharedString)>,
    current_value: SharedString,
    scrollable: bool,
    on_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> impl gpui::IntoElement {
    selector_impl(
        id,
        options,
        current_value,
        scrollable,
        SelectorLayout::Fill,
        on_change,
    )
}

enum SelectorLayout {
    Default,
    Fixed(Pixels),
    Fill,
}

fn selector_impl(
    id: &'static str,
    options: Vec<(SharedString, SharedString)>,
    current_value: SharedString,
    scrollable: bool,
    layout: SelectorLayout,
    on_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> impl gpui::IntoElement {
    let current_label = options
        .iter()
        .find(|(v, _)| *v == current_value)
        .map(|(_, l)| l.clone())
        .unwrap_or_else(|| current_value.clone());

    let on_change = Arc::new(on_change);

    let default_width = rems(10.);
    let mut button = Button::new(id)
        .child(Label::new(current_label).text_sm())
        .dropdown_caret(true)
        .outline();

    let menu_width = match layout {
        SelectorLayout::Default => {
            button = button.w(default_width);
            None
        }
        SelectorLayout::Fixed(width) => {
            button = button.w(width);
            Some(width)
        }
        SelectorLayout::Fill => {
            button = button.flex_grow(1.).min_w(rems(8.));
            None
        }
    };

    button.dropdown_menu_with_anchor(gpui::Anchor::TopLeft, move |menu, window, _| {
        let options = options.clone();
        let current_value = current_value.clone();
        let on_change = on_change.clone();
        let mut menu = menu;
        for (val, label) in options {
            let is_checked = val == current_value;
            let on_change = on_change.clone();
            let val_clone = val.clone();
            menu = menu.item(PopupMenuItem::new(label).checked(is_checked).on_click(
                move |_, window, cx| {
                    on_change(val_clone.clone(), window, cx);
                },
            ));
        }
        let min_width = menu_width.unwrap_or_else(|| default_width.to_pixels(window.rem_size()));
        menu.scrollable(scrollable).min_w(min_width)
    })
}
