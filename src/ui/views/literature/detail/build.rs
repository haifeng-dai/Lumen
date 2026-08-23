use crate::app_state::data::DataStore;
use gpui_component::Theme;
use i18n::{I18nKey, Language, t};
use models::Literature;
use std::sync::Arc;

use super::{BadgeData, TagData};

impl super::LiteratureDetailView {
    pub(super) fn build_jcr_badge(lit: &Literature, theme: &Theme) -> Option<BadgeData> {
        lit.publication
            .as_ref()
            .and_then(|p| p.jcr_rank.as_ref())
            .map(|rank| {
                let (bg, fg) = match rank.as_str() {
                    "Q1" => (theme.green, theme.primary_foreground),
                    "Q2" => (theme.blue, theme.primary_foreground),
                    "Q3" => (theme.yellow, theme.primary_foreground),
                    "Q4" => (theme.red, theme.primary_foreground),
                    _ => (theme.muted, theme.muted_foreground),
                };
                BadgeData {
                    text: format!("JCR {rank}"),
                    bg,
                    fg,
                }
            })
    }

    pub(super) fn build_ccf_badge(lit: &Literature, theme: &Theme) -> Option<BadgeData> {
        lit.publication
            .as_ref()
            .and_then(|p| p.ccf_rank.as_ref())
            .map(|rank| {
                let (bg, fg) = match rank.as_str() {
                    "A" => (theme.red, theme.primary_foreground),
                    "B" => (theme.yellow, theme.primary_foreground),
                    "C" => (theme.blue, theme.primary_foreground),
                    _ => (theme.muted, theme.muted_foreground),
                };
                BadgeData {
                    text: format!("CCF {rank}"),
                    bg,
                    fg,
                }
            })
    }

    pub(super) fn build_cas_badge(lit: &Literature, theme: &Theme) -> Option<BadgeData> {
        lit.publication
            .as_ref()
            .and_then(|p| p.cas_rank.as_ref())
            .map(|rank| {
                let (bg, fg) = if rank.contains("1区") {
                    (theme.red, theme.primary_foreground)
                } else if rank.contains("2区") {
                    (theme.yellow, theme.primary_foreground)
                } else if rank.contains("3区") {
                    (theme.blue, theme.primary_foreground)
                } else {
                    (theme.muted, theme.muted_foreground)
                };

                let display_text = if let Some(idx) = rank.find("区") {
                    if idx > 0 {
                        let 区_idx = rank.chars().take(idx + 1).count() - 1;
                        if 区_idx > 0
                            && rank.chars().nth(区_idx - 1).is_some_and(|c| c.is_numeric())
                        {
                            format!(
                                "CAS {}{}",
                                rank.chars().nth(区_idx - 1).unwrap_or(' '),
                                rank.chars().nth(区_idx).unwrap_or(' ')
                            )
                        } else {
                            format!("CAS {rank}")
                        }
                    } else {
                        format!("CAS {rank}")
                    }
                } else {
                    format!("CAS {rank}")
                };

                BadgeData {
                    text: display_text,
                    bg,
                    fg,
                }
            })
    }

    pub(super) fn build_abstract_display(&self, lit: &Literature) -> String {
        if let Some(ref text) = lit.abstract_text {
            if !self.abstract_expanded && text.chars().count() > 30 {
                let mut truncated = text.chars().take(30).collect::<String>();
                truncated.push_str("...");
                truncated
            } else {
                text.clone()
            }
        } else {
            String::new()
        }
    }

    pub(super) fn build_tags(lit: &Literature, store: &DataStore) -> Vec<TagData> {
        let fallback = store
            .tags
            .first()
            .map(|(t, _)| t.color.clone())
            .unwrap_or_default();
        lit.tags
            .iter()
            .map(|tag_name| {
                let color = store
                    .tags
                    .iter()
                    .find(|(t, _)| t.name == *tag_name)
                    .map_or_else(|| fallback.clone(), |(t, _)| t.color.clone());
                TagData {
                    name: tag_name.clone(),
                    color,
                }
            })
            .collect()
    }

    pub(super) fn build_references(
        &self,
        lit: &Literature,
        store: &DataStore,
    ) -> Vec<Arc<Literature>> {
        self.app
            .db
            .get_references(&lit.id)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| {
                store
                    .literatures
                    .iter()
                    .find(|l| l.id == c.target_id)
                    .cloned()
            })
            .collect()
    }

    pub(super) fn build_cited_by(
        &self,
        lit: &Literature,
        store: &DataStore,
    ) -> Vec<Arc<Literature>> {
        self.app
            .db
            .get_cited_by(&lit.id)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| {
                store
                    .literatures
                    .iter()
                    .find(|l| l.id == c.source_id)
                    .cloned()
            })
            .collect()
    }

    pub(super) fn build_folder_paths(
        lit: &Literature,
        store: &DataStore,
        lang: Language,
    ) -> Vec<Vec<String>> {
        lit.folder_ids
            .iter()
            .map(|folder_id| {
                let mut path = Vec::new();
                let mut current_id = Some(folder_id.clone());
                while let Some(id) = current_id {
                    if let Some(folder) = store.folders.iter().find(|f| f.id == id) {
                        path.push(folder.name.clone());
                        current_id = folder.parent_id.clone();
                    } else {
                        let name = match id.as_str() {
                            "all" => t(I18nKey::AllLiterature, lang),
                            "uncategorized" => t(I18nKey::Uncategorized, lang),
                            "trash" => t(I18nKey::Trash, lang),
                            _ => &id,
                        };
                        path.push(name.to_string());
                        current_id = None;
                    }
                }
                path.reverse();
                path
            })
            .collect()
    }
}
