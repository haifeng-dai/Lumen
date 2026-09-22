//! 全库导入编排 v2
//!
//! 文献 id 冲突则新建；作者/出版源/文件夹/标签按规则收敛；订阅不合并；
//! 附件防重名拷贝；导入后可按 filename_template 重命名。不自动查重。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use database::Database;
use log::{debug, info};
use models::LibraryImportReport;
use models::library_export::{
    AnnotationExportRow, AttachmentExportRow, AuthorExportRow, CitationExportRow, ExportManifest,
    FeedExportRow, FeedItemExportRow, FolderExportRow, ImportCounts, ImportLookupMaps,
    LibraryExportBundle, LiteratureAuthorExportRow, LiteratureExportRow, LiteratureFolderExportRow,
    LiteratureNoteExportRow, LiteratureTagExportRow, PreparedAttachmentInsert,
    PreparedLibraryImport, PublicationExportRow, TagExportRow,
};
use uuid::Uuid;

const SYSTEM_FOLDER_IDS: &[&str] = &["all", "uncategorized", "trash"];

pub fn resolve_package_root(selected: &Path) -> Result<PathBuf> {
    if selected.join("MANIFEST.json").is_file() {
        return Ok(selected.to_path_buf());
    }
    let mut best: Option<PathBuf> = None;
    let entries = fs::read_dir(selected)
        .with_context(|| format!("read export parent {}", selected.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("Lumen-Library-") || !path.join("MANIFEST.json").is_file() {
            continue;
        }
        let better = match &best {
            Some(prev) => {
                prev.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
                    < name
            }
            None => true,
        };
        if better {
            best = Some(path);
        }
    }
    best.ok_or_else(|| anyhow!("所选目录中未找到 Lumen 导出包（缺少 MANIFEST.json）"))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    if !path.is_file() {
        return Ok(T::default());
    }
    read_json(path)
}

pub fn load_export_bundle(package_root: &Path) -> Result<(ExportManifest, LibraryExportBundle)> {
    let manifest: ExportManifest = read_json(&package_root.join("MANIFEST.json"))?;
    Database::validate_import_manifest(&manifest).map_err(|m| anyhow!("导出包校验失败: {m}"))?;
    let lib = package_root.join("library");
    let bundle = LibraryExportBundle {
        literatures: read_json_or_default(&lib.join("literatures.json"))?,
        publications: read_json_or_default(&lib.join("publications.json"))?,
        authors: read_json_or_default(&lib.join("authors.json"))?,
        literature_authors: read_json_or_default(&lib.join("literature_authors.json"))?,
        folders: read_json_or_default(&lib.join("folders.json"))?,
        literature_folders: read_json_or_default(&lib.join("literature_folders.json"))?,
        tags: read_json_or_default(&lib.join("tags.json"))?,
        literature_tags: read_json_or_default(&lib.join("literature_tags.json"))?,
        attachments: read_json_or_default(&lib.join("attachments.json"))?,
        annotations: read_json_or_default(&lib.join("annotations.json"))?,
        literature_notes: read_json_or_default(&lib.join("literature_notes.json"))?,
        citations: read_json_or_default(&lib.join("citations.json"))?,
        feeds: read_json_or_default(&lib.join("feeds.json"))?,
        feed_items: read_json_or_default(&lib.join("feed_items.json"))?,
    };
    Ok((manifest, bundle))
}

fn unique_file_name(dir: &Path, file_name: &str) -> String {
    if !dir.join(file_name).exists() {
        return file_name.to_string();
    }
    let path = Path::new(file_name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let mut n = 1u32;
    loop {
        let candidate = format!("{stem}({n}){ext}");
        if !dir.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
        if n > 100_000 {
            return format!("{stem}({}){ext}", Uuid::new_v4().simple());
        }
    }
}

fn new_id_if_taken(old: &str, taken: &HashSet<String>) -> String {
    if taken.contains(old) {
        Uuid::new_v4().to_string()
    } else {
        old.to_string()
    }
}

fn values_set<K, V>(map: &HashMap<K, V>) -> HashSet<V>
where
    V: Clone + Eq + std::hash::Hash,
{
    map.values().cloned().collect()
}

fn resolve_author_id(
    a: &AuthorExportRow,
    maps: &ImportLookupMaps,
    author_map: &mut HashMap<String, String>,
    plan: &mut PreparedLibraryImport,
    counts: &mut ImportCounts,
) -> String {
    if let Some(id) = author_map.get(&a.id) {
        return id.clone();
    }
    let key = (a.first_name.clone(), a.last_name.clone());
    let taken = values_set(&maps.authors_by_name);
    let id = if let Some(local) = maps.authors_by_name.get(&key) {
        counts.authors_reused += 1;
        local.clone()
    } else {
        let nid = new_id_if_taken(&a.id, &taken);
        plan.authors.push(AuthorExportRow {
            id: nid.clone(),
            first_name: a.first_name.clone(),
            last_name: a.last_name.clone(),
            middle_name: a.middle_name.clone(),
            is_dirty: true,
            version: a.version,
            created_at: a.created_at,
            updated_at: a.updated_at,
        });
        counts.authors_inserted += 1;
        nid
    };
    author_map.insert(a.id.clone(), id.clone());
    id
}

fn resolve_publication_id(
    p: &PublicationExportRow,
    maps: &ImportLookupMaps,
    pub_map: &mut HashMap<String, String>,
    plan: &mut PreparedLibraryImport,
    counts: &mut ImportCounts,
) -> String {
    if let Some(id) = pub_map.get(&p.id) {
        return id.clone();
    }
    let taken = values_set(&maps.publications_by_name);
    let id = if let Some(local) = maps.publications_by_name.get(&p.name) {
        counts.publications_reused += 1;
        local.clone()
    } else {
        let nid = new_id_if_taken(&p.id, &taken);
        plan.publications.push(PublicationExportRow {
            id: nid.clone(),
            name: p.name.clone(),
            publication_type: p.publication_type.clone(),
            abbreviation: p.abbreviation.clone(),
            publisher: p.publisher.clone(),
            ccf_rank: p.ccf_rank.clone(),
            jcr_rank: p.jcr_rank.clone(),
            cas_rank: p.cas_rank.clone(),
            is_dirty: true,
            version: p.version,
            created_at: p.created_at,
            updated_at: p.updated_at,
        });
        counts.publications_inserted += 1;
        nid
    };
    pub_map.insert(p.id.clone(), id.clone());
    id
}

fn resolve_folder_id(
    f: &FolderExportRow,
    maps: &ImportLookupMaps,
    folder_map: &mut HashMap<String, String>,
    plan: &mut PreparedLibraryImport,
    counts: &mut ImportCounts,
) -> String {
    if let Some(id) = folder_map.get(&f.id) {
        return id.clone();
    }
    let id = if SYSTEM_FOLDER_IDS.contains(&f.id.as_str()) {
        let local = maps
            .folders_by_name
            .get(&f.id)
            .or_else(|| maps.folders_by_name.get(&f.name))
            .cloned()
            .unwrap_or_else(|| f.id.clone());
        counts.folders_reused += 1;
        local
    } else if let Some(local) = maps
        .folders_by_name
        .get(&f.name)
        .or_else(|| maps.folders_by_name.get(&f.id))
    {
        counts.folders_reused += 1;
        local.clone()
    } else {
        let taken = values_set(&maps.folders_by_name);
        let nid = new_id_if_taken(&f.id, &taken);
        plan.folders.push(FolderExportRow {
            id: nid.clone(),
            name: f.name.clone(),
            folder_type: f.folder_type.clone(),
            parent_id: f.parent_id.clone(),
            is_dirty: true,
            version: f.version,
            created_at: f.created_at,
            updated_at: f.updated_at,
        });
        counts.folders_inserted += 1;
        nid
    };
    folder_map.insert(f.id.clone(), id.clone());
    id
}

fn resolve_tag_id(
    t: &TagExportRow,
    maps: &ImportLookupMaps,
    tag_map: &mut HashMap<String, String>,
    plan: &mut PreparedLibraryImport,
    counts: &mut ImportCounts,
) -> String {
    if let Some(id) = tag_map.get(&t.id) {
        return id.clone();
    }
    let id = if let Some(local) = maps.tags_by_name.get(&t.name) {
        counts.tags_reused += 1;
        local.clone()
    } else {
        let taken = values_set(&maps.tags_by_name);
        let nid = new_id_if_taken(&t.id, &taken);
        plan.tags.push(TagExportRow {
            id: nid.clone(),
            name: t.name.clone(),
            color: t.color.clone(),
            is_dirty: true,
            version: t.version,
            created_at: t.created_at,
            updated_at: t.updated_at,
        });
        counts.tags_inserted += 1;
        nid
    };
    tag_map.insert(t.id.clone(), id.clone());
    id
}

/// 规划导入：词表收敛 + 主键映射 + 外键重写（不拷贝文件）
pub fn plan_import(
    bundle: &LibraryExportBundle,
    maps: &ImportLookupMaps,
) -> (PreparedLibraryImport, ImportCounts) {
    let mut plan = PreparedLibraryImport::default();
    let mut counts = ImportCounts::default();

    let mut lit_map: HashMap<String, String> = HashMap::new();
    let mut author_map: HashMap<String, String> = HashMap::new();
    let mut pub_map: HashMap<String, String> = HashMap::new();
    let mut folder_map: HashMap<String, String> = HashMap::new();
    let mut tag_map: HashMap<String, String> = HashMap::new();
    let mut feed_map: HashMap<String, String> = HashMap::new();
    let mut att_map: HashMap<String, String> = HashMap::new();

    let authors_by_id: HashMap<&str, &AuthorExportRow> =
        bundle.authors.iter().map(|a| (a.id.as_str(), a)).collect();
    let pubs_by_id: HashMap<&str, &PublicationExportRow> = bundle
        .publications
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let folders_by_id: HashMap<&str, &FolderExportRow> =
        bundle.folders.iter().map(|f| (f.id.as_str(), f)).collect();
    let tags_by_id: HashMap<&str, &TagExportRow> =
        bundle.tags.iter().map(|t| (t.id.as_str(), t)).collect();

    // feeds：不合并，id 冲突则新建
    for f in &bundle.feeds {
        let new_id = new_id_if_taken(&f.id, &maps.existing_feed_ids);
        if !maps.existing_feed_ids.contains(&f.id) || new_id != f.id {
            plan.feeds.push(FeedExportRow {
                id: new_id.clone(),
                name: f.name.clone(),
                title: f.title.clone(),
                feed_type: f.feed_type.clone(),
                url: f.url.clone(),
                last_updated_at: f.last_updated_at.clone(),
                update_interval: f.update_interval,
                is_dirty: true,
                version: f.version,
                created_at: f.created_at,
                updated_at: f.updated_at,
            });
            counts.feeds_inserted += 1;
        }
        feed_map.insert(f.id.clone(), new_id);
    }

    for item in &bundle.feed_items {
        let parent = feed_map
            .get(&item.feed_id)
            .cloned()
            .unwrap_or_else(|| item.feed_id.clone());
        let new_id = new_id_if_taken(&item.id, &maps.existing_feed_item_ids);
        plan.feed_items.push(FeedItemExportRow {
            id: new_id,
            title: item.title.clone(),
            feed_id: parent,
            is_read: item.is_read,
            is_added_to_library: item.is_added_to_library,
            added_at: item.added_at.clone(),
            authors: item.authors.clone(),
            year: item.year,
            literature_type: item.literature_type.clone(),
            journal: item.journal.clone(),
            publisher: item.publisher.clone(),
            abstract_text: item.abstract_text.clone(),
            doi: item.doi.clone(),
            url: item.url.clone(),
            volume: item.volume.clone(),
            issue: item.issue.clone(),
            pages: item.pages.clone(),
            published_at: item.published_at.clone(),
            is_dirty: true,
            version: item.version,
            updated_at: item.updated_at,
        });
        counts.feed_items_inserted += 1;
    }

    // ── 词表全量 resolve（缺口 A）────────────────────────────
    for f in &bundle.folders {
        resolve_folder_id(f, maps, &mut folder_map, &mut plan, &mut counts);
    }
    // parent_id 在 folder_map 填满后重写（缺口 B）
    for f in plan.folders.iter_mut() {
        if let Some(pid) = f.parent_id.clone() {
            f.parent_id = folder_map
                .get(&pid)
                .cloned()
                .or_else(|| maps.folders_by_name.get(&pid).cloned());
        }
    }
    for t in &bundle.tags {
        resolve_tag_id(t, maps, &mut tag_map, &mut plan, &mut counts);
    }

    // literatures：id 冲突一律新建
    for lit in &bundle.literatures {
        let new_id = if maps.existing_literature_ids.contains(&lit.id) {
            counts.literatures_new_id += 1;
            Uuid::new_v4().to_string()
        } else {
            counts.literatures_reused_id += 1;
            lit.id.clone()
        };
        let publication_id = lit
            .publication_id
            .as_ref()
            .and_then(|pid| pubs_by_id.get(pid.as_str()))
            .map(|p| resolve_publication_id(p, maps, &mut pub_map, &mut plan, &mut counts));
        plan.literatures.push(LiteratureExportRow {
            id: new_id.clone(),
            title: lit.title.clone(),
            year: lit.year,
            month: lit.month,
            day: lit.day,
            literature_type: lit.literature_type.clone(),
            publication_id,
            volume: lit.volume.clone(),
            issue: lit.issue.clone(),
            pages: lit.pages.clone(),
            abstract_text: lit.abstract_text.clone(),
            doi: lit.doi.clone(),
            arxiv_id: lit.arxiv_id.clone(),
            url: lit.url.clone(),
            rating: lit.rating,
            reading_status: lit.reading_status.clone(),
            is_dirty: true,
            version: lit.version,
            created_at: lit.created_at,
            updated_at: lit.updated_at,
        });
        lit_map.insert(lit.id.clone(), new_id);
    }
    plan.imported_literature_ids = bundle
        .literatures
        .iter()
        .filter_map(|l| lit_map.get(&l.id).cloned())
        .collect();

    for r in &bundle.literature_authors {
        let Some(new_lit) = lit_map.get(&r.literature_id) else {
            continue;
        };
        let Some(auth) = authors_by_id.get(r.author_id.as_str()) else {
            counts.relations_skipped += 1;
            continue;
        };
        let author_id = resolve_author_id(auth, maps, &mut author_map, &mut plan, &mut counts);
        plan.literature_authors.push(LiteratureAuthorExportRow {
            literature_id: new_lit.clone(),
            author_id,
            sort_order: r.sort_order,
            is_dirty: true,
            version: r.version,
            updated_at: r.updated_at,
        });
    }
    for r in &bundle.literature_folders {
        let Some(new_lit) = lit_map.get(&r.literature_id) else {
            continue;
        };
        let Some(folder) = folders_by_id.get(r.folder_id.as_str()) else {
            counts.relations_skipped += 1;
            continue;
        };
        let folder_id = resolve_folder_id(folder, maps, &mut folder_map, &mut plan, &mut counts);
        plan.literature_folders.push(LiteratureFolderExportRow {
            literature_id: new_lit.clone(),
            folder_id,
            is_dirty: true,
            version: r.version,
            updated_at: r.updated_at,
        });
    }
    for r in &bundle.literature_tags {
        let Some(new_lit) = lit_map.get(&r.literature_id) else {
            continue;
        };
        let Some(tag) = tags_by_id.get(r.tag_id.as_str()) else {
            counts.relations_skipped += 1;
            continue;
        };
        let tag_id = resolve_tag_id(tag, maps, &mut tag_map, &mut plan, &mut counts);
        plan.literature_tags.push(LiteratureTagExportRow {
            literature_id: new_lit.clone(),
            tag_id,
            is_dirty: true,
            version: r.version,
            updated_at: r.updated_at,
        });
    }

    for att in &bundle.attachments {
        let Some(new_lit) = lit_map.get(&att.literature_id) else {
            continue;
        };
        let new_att_id = new_id_if_taken(&att.id, &maps.existing_attachment_ids);
        att_map.insert(att.id.clone(), new_att_id.clone());
        plan.attachments.push(PreparedAttachmentInsert {
            source_package_id: att.id.clone(),
            id: new_att_id,
            literature_id: new_lit.clone(),
            file_path: String::new(),
            file_name: att.file_name.clone(),
            file_size: att.file_size,
            mime_type: att.mime_type.clone(),
            etag: att.etag.clone(),
            hash: att.hash.clone(),
            is_main: att.is_main,
            version: att.version,
            created_at: att.created_at,
            updated_at: att.updated_at,
        });
    }

    for n in &bundle.literature_notes {
        let Some(new_lit) = lit_map.get(&n.literature_id) else {
            continue;
        };
        let new_id = new_id_if_taken(&n.id, &maps.existing_note_ids);
        plan.literature_notes.push(LiteratureNoteExportRow {
            id: new_id,
            literature_id: new_lit.clone(),
            title: n.title.clone(),
            content: n.content.clone(),
            sort_order: n.sort_order,
            is_dirty: true,
            version: n.version,
            created_at: n.created_at,
            updated_at: n.updated_at,
        });
    }

    // 批注：PDF 阅读器使用 document_id = "{literature_id}::{attachment_id}"
    // 包内可能是 att_id 或 lit::att；导入时统一写成映射后的 lit::att
    let att_lit_by_id: HashMap<String, String> = plan
        .attachments
        .iter()
        .map(|a| (a.id.clone(), a.literature_id.clone()))
        .collect();

    for ann in &bundle.annotations {
        let raw_doc = ann.document_id.as_str();
        let att_key = raw_doc.rsplit("::").next().unwrap_or(raw_doc);
        let Some(new_att) = att_map.get(att_key) else {
            counts.annotations_skipped += 1;
            continue;
        };
        let Some(new_lit) = att_lit_by_id.get(new_att) else {
            counts.annotations_skipped += 1;
            continue;
        };
        let new_id = new_id_if_taken(&ann.id, &maps.existing_annotation_ids);
        plan.annotations.push(AnnotationExportRow {
            id: new_id,
            document_id: format!("{new_lit}::{new_att}"),
            page: ann.page,
            kind: ann.kind.clone(),
            color: ann.color.clone(),
            range: ann.range.clone(),
            note: ann.note.clone(),
            created_at: ann.created_at,
            updated_at: ann.updated_at,
            version: ann.version,
        });
    }

    for c in &bundle.citations {
        let (Some(src), Some(tgt)) = (lit_map.get(&c.source_id), lit_map.get(&c.target_id)) else {
            counts.citations_skipped += 1;
            continue;
        };
        plan.citations.push(CitationExportRow {
            source_id: src.clone(),
            target_id: tgt.clone(),
            is_dirty: true,
            version: c.version,
            updated_at: c.updated_at,
        });
    }

    (plan, counts)
}

fn copy_attachments(
    package_root: &Path,
    bundle: &LibraryExportBundle,
    plan: &mut PreparedLibraryImport,
    attachments_dir: &Path,
    counts: &mut ImportCounts,
) -> Result<()> {
    let src_by_pkg_id: HashMap<&str, &AttachmentExportRow> = bundle
        .attachments
        .iter()
        .map(|a| (a.id.as_str(), a))
        .collect();

    let mut next_src_name: HashMap<String, usize> = HashMap::new();

    for planned in plan.attachments.iter_mut() {
        let Some(src) = src_by_pkg_id.get(planned.source_package_id.as_str()) else {
            counts.attachments_file_missing += 1;
            planned.file_path.clear();
            continue;
        };
        let src_path = if !src.export_relative_path.is_empty() {
            package_root.join(&src.export_relative_path)
        } else {
            package_root
                .join("attachments")
                .join(&src.literature_id)
                .join(&src.file_name)
        };
        if !src_path.is_file() {
            counts.attachments_file_missing += 1;
            planned.file_path.clear();
            continue;
        }
        let dest_dir = attachments_dir.join(&planned.literature_id);
        fs::create_dir_all(&dest_dir)?;
        // 同目录批量导入时按「包内原名 → 若占用则 stem(n)」
        let mut final_name = src.file_name.clone();
        let already = next_src_name
            .entry(format!("{}|{}", planned.literature_id, src.file_name))
            .or_insert(0);
        if *already > 0 || dest_dir.join(&final_name).exists() {
            final_name = unique_file_name(&dest_dir, &src.file_name);
            if final_name != src.file_name {
                counts.attachments_renamed_on_copy += 1;
            }
        }
        *already += 1;
        let dest = dest_dir.join(&final_name);
        if dest.exists() {
            final_name = unique_file_name(&dest_dir, &src.file_name);
            counts.attachments_renamed_on_copy += 1;
        }
        let dest = dest_dir.join(&final_name);
        fs::copy(&src_path, &dest).with_context(|| format!("copy attachment {final_name}"))?;
        let size = fs::metadata(&dest)
            .map(|m| m.len())
            .unwrap_or(src.file_size);
        planned.file_path = dest.to_string_lossy().to_string();
        planned.file_name = final_name;
        planned.file_size = size;
    }

    plan.attachments.retain(|a| !a.file_path.is_empty());
    Ok(())
}

fn rename_imported_attachments(
    db: &Database,
    lit_ids: &[String],
    template: &str,
    counts: &mut ImportCounts,
) {
    let service = crate::library::LiteratureService::new();
    for lit_id in lit_ids {
        let Ok(Some(mut lit)) = db.get_literature(lit_id) else {
            continue;
        };
        if lit.attachments.is_empty() {
            continue;
        }
        let stats = service.rename_local_attachments(&mut lit, template);
        counts.attachments_template_renamed += stats.success;
        counts.attachments_template_rename_failed += stats.failures.len();
        if stats.success > 0
            && let Err(e) = db.insert_literature(&lit)
        {
            debug!("导入: 模板重命名后保存失败 lit={lit_id}: {e}");
            counts.attachments_template_rename_failed += 1;
        }
    }
}

/// 导入导出包到当前库。
///
/// `filename_template` 非空时，导入后对本次文献按模板重命名附件并回写关联。
pub fn import_library(
    db: &Database,
    attachments_dir: &Path,
    selected: &Path,
    filename_template: &str,
) -> Result<LibraryImportReport> {
    let package_root = resolve_package_root(selected)?;
    info!("导入: v2 合并导入 package_root={}", package_root.display());

    let (_manifest, bundle) = load_export_bundle(&package_root)?;
    let maps = db.import_lookup_maps()?;
    let (mut plan, mut counts) = plan_import(&bundle, &maps);
    copy_attachments(
        &package_root,
        &bundle,
        &mut plan,
        attachments_dir,
        &mut counts,
    )?;

    let written = db.import_prepared_library(&plan)?;
    counts.attachments_inserted = written.attachments_inserted;
    counts.annotations_inserted = written.annotations_inserted;
    counts.literature_notes_inserted = written.literature_notes_inserted;
    counts.citations_inserted = written.citations_inserted;

    if !filename_template.trim().is_empty() {
        rename_imported_attachments(
            db,
            &plan.imported_literature_ids,
            filename_template,
            &mut counts,
        );
    }

    info!(
        "导入: 完成 lit_reused={} lit_new={} att={} template_rename={} missing={}",
        counts.literatures_reused_id,
        counts.literatures_new_id,
        counts.attachments_inserted,
        counts.attachments_template_renamed,
        counts.attachments_file_missing
    );

    Ok(LibraryImportReport {
        package_root: package_root.to_string_lossy().to_string(),
        counts,
        imported_literature_ids: plan.imported_literature_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::export_library;
    use models::LiteratureType;
    use models::constructors::{create_attachment, create_literature};

    #[test]
    fn resolve_package_root_from_parent_and_self() {
        let dir = std::env::temp_dir().join(format!("lumen-import-res-{}", Uuid::new_v4()));
        let atts = dir.join("atts");
        let parent = dir.join("backups");
        fs::create_dir_all(&atts).unwrap();
        let db = Database::new(":memory:").unwrap();
        db.insert_literature(&create_literature("lit-r", "R", LiteratureType::Article))
            .unwrap();
        let report = export_library(&db, &atts, &parent).unwrap();
        let pkg = PathBuf::from(&report.dest_dir);
        assert_eq!(resolve_package_root(&parent).unwrap(), pkg);
        assert_eq!(resolve_package_root(&pkg).unwrap(), pkg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unique_file_name_adds_counter_suffix() {
        let dir = std::env::temp_dir().join(format!("lumen-fname-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("paper.pdf"), b"a").unwrap();
        assert_eq!(unique_file_name(&dir, "paper.pdf"), "paper(1).pdf");
        fs::write(dir.join("paper(1).pdf"), b"b").unwrap();
        assert_eq!(unique_file_name(&dir, "paper.pdf"), "paper(2).pdf");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_conflict_new_literature_reuses_author() {
        let dir = std::env::temp_dir().join(format!("lumen-import-v2-{}", Uuid::new_v4()));
        let src_atts = dir.join("src-atts");
        let parent = dir.join("backups");
        let dest_atts = dir.join("dest-atts");
        fs::create_dir_all(&src_atts).unwrap();
        let pdf = src_atts.join("paper.pdf");
        fs::write(&pdf, b"%PDF-1.4 x").unwrap();

        let src = Database::new(":memory:").unwrap();
        let mut lit = create_literature("lit-conflict", "Paper", LiteratureType::Article);
        lit.doi = Some("10.1/x".into());
        lit.authors = vec![models::Author {
            id: "auth-pkg".into(),
            first_name: "Ada".into(),
            last_name: "Lovelace".into(),
            middle_name: None,
            is_dirty: true,
            is_deleted: false,
            version: 1,
            created_at: 1,
            updated_at: 1,
        }];
        src.insert_literature(&lit).unwrap();
        let mut att = create_attachment(
            "att-pkg".into(),
            "lit-conflict".into(),
            pdf.to_string_lossy().into_owned(),
            "paper.pdf".into(),
            10,
        );
        att.is_main = true;
        src.insert_attachment(&att).unwrap();
        export_library(&src, &src_atts, &parent).unwrap();

        let dest = Database::new(":memory:").unwrap();
        dest.insert_literature(&create_literature(
            "lit-conflict",
            "LocalTitle",
            LiteratureType::Book,
        ))
        .unwrap();
        dest.insert_literature(&{
            let mut other = create_literature("lit-holder", "Holder", LiteratureType::Article);
            other.authors = vec![models::Author {
                id: "auth-local".into(),
                first_name: "Ada".into(),
                last_name: "Lovelace".into(),
                middle_name: None,
                is_dirty: false,
                is_deleted: false,
                version: 1,
                created_at: 1,
                updated_at: 1,
            }];
            other
        })
        .unwrap();

        let report = import_library(&dest, &dest_atts, &parent, "").unwrap();
        assert_eq!(report.counts.literatures_new_id, 1);
        assert!(report.counts.authors_reused >= 1);

        let local = dest.get_literature("lit-conflict").unwrap().unwrap();
        assert_eq!(local.title, "LocalTitle");

        let all = dest.get_all_literatures().unwrap();
        let imported = all
            .iter()
            .find(|l| l.title == "Paper")
            .expect("imported paper");
        assert_ne!(imported.id, "lit-conflict");
        assert!(
            imported.authors.iter().any(|a| a.id == "auth-local"),
            "authors={:?}",
            imported.authors
        );
        assert!(
            imported
                .attachments
                .iter()
                .any(|a| Path::new(&a.file_path).is_file()),
            "atts={:?}",
            imported.attachments
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreferenced_folder_and_tag_import_and_parent_id_remap() {
        let dir = std::env::temp_dir().join(format!("lumen-vocab-{}", Uuid::new_v4()));
        let src_atts = dir.join("src-atts");
        let parent = dir.join("backups");
        let dest_atts = dir.join("dest-atts");
        fs::create_dir_all(&src_atts).unwrap();

        let src = Database::new(":memory:").unwrap();
        // 空自定义夹 + 父子夹 + 无引用标签
        let parent_folder = models::Folder {
            id: "fold-parent".into(),
            name: "ParentFolder".into(),
            folder_type: models::FolderType::Custom,
            parent_id: None,
            literature_count: 0,
            is_dirty: false,
            is_deleted: false,
            version: 1,
            created_at: 1,
            updated_at: 1,
        };
        let child_folder = models::Folder {
            id: "fold-child".into(),
            name: "ChildFolder".into(),
            folder_type: models::FolderType::Custom,
            parent_id: Some("fold-parent".into()),
            literature_count: 0,
            is_dirty: false,
            is_deleted: false,
            version: 1,
            created_at: 1,
            updated_at: 1,
        };
        src.insert_folder(&parent_folder).unwrap();
        src.insert_folder(&child_folder).unwrap();
        src.create_tag("orphan-tag", Some("#123456".into()))
            .unwrap();
        src.insert_literature(&create_literature("lit-v", "V", LiteratureType::Article))
            .unwrap();
        export_library(&src, &src_atts, &parent).unwrap();

        let dest = Database::new(":memory:").unwrap();
        // 目标库：同名父夹但不同 id → 应复用本地 parent，并映射 child.parent_id
        let local_parent = models::Folder {
            id: "fold-local".into(),
            name: "ParentFolder".into(),
            folder_type: models::FolderType::Custom,
            parent_id: None,
            literature_count: 0,
            is_dirty: false,
            is_deleted: false,
            version: 1,
            created_at: 1,
            updated_at: 1,
        };
        dest.insert_folder(&local_parent).unwrap();

        let report = import_library(&dest, &dest_atts, &parent, "").unwrap();
        assert!(report.counts.tags_inserted >= 1 || report.counts.tags_reused >= 1);

        let folders = dest.get_all_folders().unwrap();
        let orphan = folders.iter().find(|f| f.name == "orphan-tag");
        let _ = orphan;
        let tag = dest
            .get_all_tags_with_counts()
            .unwrap()
            .into_iter()
            .find(|(t, _)| t.name == "orphan-tag")
            .map(|(t, _)| t);
        assert!(tag.is_some(), "unreferenced tag must be imported");

        let child = folders.iter().find(|f| f.name == "ChildFolder");
        assert!(
            child.is_some(),
            "unreferenced child folder must be imported"
        );
        let child = child.unwrap();
        assert_eq!(
            child.parent_id.as_deref(),
            Some("fold-local"),
            "parent_id must map to local folder id, got {:?}",
            child.parent_id
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn annotation_document_id_uses_lit_att_composite() {
        let dir = std::env::temp_dir().join(format!("lumen-ann-{}", Uuid::new_v4()));
        let src_atts = dir.join("src-atts");
        let parent = dir.join("backups");
        let dest_atts = dir.join("dest-atts");
        fs::create_dir_all(&src_atts).unwrap();
        let pdf = src_atts.join("p.pdf");
        fs::write(&pdf, b"%PDF").unwrap();

        let src = Database::new(":memory:").unwrap();
        let lit = create_literature("lit-a", "A", LiteratureType::Article);
        src.insert_literature(&lit).unwrap();
        let mut att = create_attachment(
            "att-a".into(),
            "lit-a".into(),
            pdf.to_string_lossy().into_owned(),
            "p.pdf".into(),
            4,
        );
        att.is_main = true;
        src.insert_attachment(&att).unwrap();
        src.save_annotation(&models::Annotation {
            id: "ann-a".into(),
            // 阅读器运行时键
            document_id: "lit-a::att-a".into(),
            page: 1,
            kind: models::AnnotationKind::Highlight,
            color: models::AnnotationColor::Yellow,
            range: None,
            note: Some("n".into()),
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_dirty: false,
        })
        .unwrap();
        // 文献笔记
        let note_id = src.create_note("lit-a", "note-title").unwrap();
        export_library(&src, &src_atts, &parent).unwrap();

        let dest = Database::new(":memory:").unwrap();
        let report = import_library(&dest, &dest_atts, &parent, "").unwrap();
        assert_eq!(report.counts.annotations_inserted, 1);
        assert_eq!(report.counts.literature_notes_inserted, 1);

        let imported_lit_id = report
            .imported_literature_ids
            .first()
            .cloned()
            .expect("imported lit");
        let anns = dest
            .load_annotations(&format!("{imported_lit_id}::att-a"))
            .unwrap();
        // att id may be remapped if conflict; search by note
        let notes = dest.list_notes(&imported_lit_id).unwrap();
        assert!(!notes.is_empty(), "notes should follow literature");
        assert_eq!(notes[0].title, "note-title");
        let _ = note_id;

        // Multi-key load should find annotation even if stored as composite
        let persistence = crate::library::PdfPersistence::new();
        let all_att = persistence.load_annotations(&dest, "att-a");
        let all_composite =
            persistence.load_annotations(&dest, &format!("{imported_lit_id}::att-a"));
        assert!(
            !all_att.is_empty() || !all_composite.is_empty(),
            "annotation must be loadable via viewer key or att id; composite={all_composite:?} att={all_att:?} anns_db={anns:?}"
        );
        if let Some(a) = all_composite.first().or(all_att.first()) {
            assert_eq!(a.note.as_deref(), Some("n"));
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tag_name_conflict_reuses_local_color() {
        let dir = std::env::temp_dir().join(format!("lumen-tag-{}", Uuid::new_v4()));
        let src_atts = dir.join("src-atts");
        let parent = dir.join("backups");
        let dest_atts = dir.join("dest-atts");
        fs::create_dir_all(&src_atts).unwrap();

        let src = Database::new(":memory:").unwrap();
        let mut lit = create_literature("lit-tag", "T", LiteratureType::Article);
        lit.tags = vec!["ml".into()];
        src.create_tag("ml", Some("#ff0000".into())).unwrap();
        src.insert_literature(&lit).unwrap();
        export_library(&src, &src_atts, &parent).unwrap();

        let dest = Database::new(":memory:").unwrap();
        let local_tag = dest.create_tag("ml", Some("#00ff00".into())).unwrap();
        let report = import_library(&dest, &dest_atts, &parent, "").unwrap();
        assert_eq!(report.counts.tags_reused, 1);
        assert_eq!(report.counts.tags_inserted, 0);

        let tags = dest.get_all_tags_with_counts().unwrap();
        let ml = tags.iter().find(|(t, _)| t.name == "ml").unwrap().0.clone();
        assert_eq!(ml.id, local_tag.id);
        assert_eq!(ml.color, "#00ff00");
        let _ = fs::remove_dir_all(&dir);
    }
}
