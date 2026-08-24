use crate::app_state::ui::UiState;
use services::query::data::AppViewMode;

use super::*;

impl super::MainWindow {
    pub fn select_folder(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            state.selected_folder_id = Some(id.clone());
            state.selected_tag_id = None;
            state.selected_literature_ids.clear();
        });
        if let Ok(mut state) = self.app.local_state.write() {
            state.selected_sidebar_item = Some(format!("folder:{id}"));
        }
        self.literature_list.update(cx, |list, cx| {
            list.refresh_visible_literatures(cx);
        });
    }
    pub fn select_tag(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            state.selected_tag_id = Some(id.clone());
            state.selected_folder_id = None;
            state.selected_literature_ids.clear();
        });
        if let Ok(mut state) = self.app.local_state.write() {
            state.selected_sidebar_item = Some(format!("tag:{id}"));
        }
        self.literature_list.update(cx, |list, cx| {
            list.refresh_visible_literatures(cx);
        });
    }
    pub fn select_literature(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            state.selected_literature_ids.clear();
            state.selected_literature_ids.insert(id);
        });
    }
    pub fn toggle_literature_selection(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            if state.selected_literature_ids.contains(&id) {
                state.selected_literature_ids.remove(&id);
            } else {
                state.selected_literature_ids.insert(id);
            }
        });
    }
    pub fn add_literature_selection(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            state.selected_literature_ids.insert(id);
        });
    }
    pub fn select_feed(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            state.selected_feed_id = Some(id);
            state.selected_feed_item_ids.clear();
        });
    }
    pub fn select_feed_item(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            state.selected_feed_item_ids.clear();
            state.selected_feed_item_ids.insert(id);
        });
    }
    pub fn toggle_feed_item_selection(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            if state.selected_feed_item_ids.contains(&id) {
                state.selected_feed_item_ids.remove(&id);
            } else {
                state.selected_feed_item_ids.insert(id);
            }
        });
    }
    pub fn add_feed_item_selection(&mut self, id: String, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            state.selected_feed_item_ids.insert(id);
        });
    }
    pub fn set_view_mode(&mut self, mode: AppViewMode, cx: &mut Context<Self>) {
        UiState::update(cx, |state| {
            state.view_mode = mode;
        });
        // 视图模式切换时，重建原生菜单栏（文献库 / 订阅 菜单随模式变化）
        let lang = self.app.current_language();
        cx.set_menus(build_app_menus(mode, lang));
    }
}
