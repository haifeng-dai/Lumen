use gpui::prelude::*;
use gpui_component::ActiveTheme;
use log::debug;
use parser::normalize::author_full_name;

use super::{DetailMode, SingleDetailBuffer};

impl super::LiteratureDetailView {
    pub(super) fn sync_state(&mut self, cx: &mut Context<Self>) {
        if !self.sync_detect_changes(cx) {
            return;
        }
        self.sync_update_mode(cx);
        cx.notify();
    }

    fn sync_detect_changes(&mut self, cx: &Context<Self>) -> bool {
        let ui = cx.global::<crate::app_state::ui::UiState>();
        let store = self.data_store.read(cx);
        let current_selected: Vec<String> = ui.selected_literature_ids.iter().cloned().collect();
        let selected_count = current_selected.len();

        let ids_changed = self.state.selected_ids != current_selected;

        let version_changed = if selected_count == 1 {
            current_selected
                .first()
                .and_then(|id| store.literatures.iter().find(|l| l.id == *id))
                .is_none_or(|lit| lit.version != self.state.content_version)
        } else {
            false
        };

        let tags_changed = if let DetailMode::Single(ref buffer) = self.state.mode {
            buffer.tags.iter().any(|tag_data| {
                store
                    .tags
                    .iter()
                    .find(|(t, _)| t.name == tag_data.name)
                    .is_none_or(|(t, _)| t.color != tag_data.color)
            })
        } else {
            false
        };

        if !ids_changed && !version_changed && !tags_changed {
            return false;
        }

        debug!(
            "详情: 检测到变化 (ids={ids_changed}, version={version_changed}, tags={tags_changed})"
        );
        self.state.selected_ids = current_selected;
        true
    }

    fn sync_update_mode(&mut self, cx: &Context<Self>) {
        let selected_count = self.state.selected_ids.len();
        if selected_count == 0 {
            self.state.mode = DetailMode::None;
            self.state.content_version = -1;
        } else if selected_count > 1 {
            self.state.mode = DetailMode::Multiple(selected_count);
            self.state.content_version = -1;
        } else if let Some(buffer) = self.sync_build_buffer(cx) {
            self.state.content_version = buffer.literature.version;
            self.state.mode = DetailMode::Single(Box::new(buffer));
            let lit_id = &self.state.selected_ids[0];
            let notes = self.app.literature_service.list_notes(&self.app.db, lit_id);
            self.notes_cache = notes;
        } else {
            self.state.mode = DetailMode::None;
        }
        debug!("详情: 模式切换 -> {} 个选中", selected_count);
    }

    fn sync_build_buffer(&self, cx: &Context<Self>) -> Option<SingleDetailBuffer> {
        let store = self.data_store.read(cx);
        let theme = cx.theme().clone();
        let first_id = self.state.selected_ids.first()?;
        let lit = store
            .literatures
            .iter()
            .find(|l| l.id == *first_id)
            .cloned()?;

        let authors_text = lit
            .authors
            .iter()
            .map(author_full_name)
            .collect::<Vec<_>>()
            .join(", ");

        let pub_name = lit
            .publication
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let pub_abbreviation = lit
            .publication
            .as_ref()
            .and_then(|p| p.abbreviation.as_ref())
            .cloned()
            .unwrap_or_default();
        let jcr_badge = Self::build_jcr_badge(&lit, &theme);
        let ccf_badge = Self::build_ccf_badge(&lit, &theme);
        let cas_badge = Self::build_cas_badge(&lit, &theme);
        let abstract_display = self.build_abstract_display(&lit);
        let tags = Self::build_tags(&lit, store);
        let references = self.build_references(&lit, store);
        let cited_by = self.build_cited_by(&lit, store);
        let folder_paths = Self::build_folder_paths(&lit, store, self.app.current_language());

        debug!(
            "详情: 构建缓冲完毕 (title='{}', authors={}, tags={}, refs={}, cited={})",
            lit.title,
            lit.authors.len(),
            lit.tags.len(),
            references.len(),
            cited_by.len()
        );

        Some(SingleDetailBuffer {
            literature: lit.clone(),
            ccf_badge,
            jcr_badge,
            cas_badge,
            authors_text,
            pub_name,
            pub_abbreviation,
            abstract_display,
            rating: lit.rating,
            tags,
            references,
            cited_by,
            reading_status: lit.reading_status,
            folder_paths,
        })
    }
}
