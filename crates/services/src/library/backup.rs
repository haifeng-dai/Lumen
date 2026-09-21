//! 全库导出编排
//!
//! 从 database 读快照 → 过滤有效闭包 / 附件文件存在性 → 写逻辑 JSON 包。
//! 不做导入；不依赖 gpui。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use database::Database;
use log::{debug, info};
use models::library_export::{
    AnnotationExportRow, AttachmentExportRow, ExportCounts, LIBRARY_EXPORT_FORMAT_VERSION,
    LibraryExportBundle, LibraryExportReport, LibraryExportSnapshot,
};

/// 将批注 `document_id` 规范为已导出附件 id。
///
/// 兼容历史键：`att_id`、`lit_id::att_id`。
pub fn normalize_document_id(raw: &str, exported_att_ids: &HashSet<String>) -> Option<String> {
    if exported_att_ids.contains(raw) {
        return Some(raw.to_string());
    }
    if let Some((_, suffix)) = raw.rsplit_once("::")
        && exported_att_ids.contains(suffix)
    {
        return Some(suffix.to_string());
    }
    None
}

fn resolve_attachment_source(
    raw_path: &str,
    file_name: &str,
    attachments_dir: &Path,
) -> Option<PathBuf> {
    let primary = PathBuf::from(raw_path);
    if primary.is_file() {
        return Some(primary);
    }
    let fallback = attachments_dir.join(file_name);
    if fallback.is_file() {
        return Some(fallback);
    }
    None
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn filter_bundle(
    snap: LibraryExportSnapshot,
    attachments_dir: &Path,
    dest_dir: &Path,
) -> Result<(LibraryExportBundle, ExportCounts)> {
    let mut counts = ExportCounts::default();
    let mut bundle = LibraryExportBundle::default();

    let lit_ids: HashSet<String> = snap.literatures.iter().map(|l| l.id.clone()).collect();

    // Folders / tags / built-ins: export all active
    bundle.folders = snap.folders;
    bundle.tags = snap.tags;

    // Publications / authors referenced by exported literatures
    let pub_ids: HashSet<String> = snap
        .literatures
        .iter()
        .filter_map(|l| l.publication_id.clone())
        .collect();
    bundle.publications = snap
        .publications
        .into_iter()
        .filter(|p| pub_ids.contains(&p.id))
        .collect();

    bundle.literature_authors = snap
        .literature_authors
        .into_iter()
        .filter(|r| lit_ids.contains(&r.literature_id))
        .collect();
    let author_ids: HashSet<String> = bundle
        .literature_authors
        .iter()
        .map(|r| r.author_id.clone())
        .collect();
    bundle.authors = snap
        .authors
        .into_iter()
        .filter(|a| author_ids.contains(&a.id))
        .collect();

    bundle.literature_folders = snap
        .literature_folders
        .into_iter()
        .filter(|r| lit_ids.contains(&r.literature_id))
        .collect();
    bundle.literature_tags = snap
        .literature_tags
        .into_iter()
        .filter(|r| lit_ids.contains(&r.literature_id))
        .collect();

    bundle.literatures = snap.literatures;

    // Attachments: only when file exists
    let attachments_root = dest_dir.join("attachments");
    let mut exported_att_ids: HashSet<String> = HashSet::new();
    for raw in snap.attachments_raw {
        if !lit_ids.contains(&raw.literature_id) {
            counts.attachments_skipped_missing_file += 1;
            continue;
        }
        let Some(src) = resolve_attachment_source(&raw.file_path, &raw.file_name, attachments_dir)
        else {
            counts.attachments_skipped_missing_file += 1;
            debug!("导出: 附件文件缺失，跳过 (category=attachment_missing)");
            continue;
        };
        let rel = format!("attachments/{}/{}", raw.literature_id, raw.file_name);
        let dest_path = dest_dir.join(&rel);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&src, &dest_path).with_context(|| format!("copy attachment {}", raw.file_name))?;
        exported_att_ids.insert(raw.id.clone());
        bundle.attachments.push(AttachmentExportRow {
            id: raw.id,
            literature_id: raw.literature_id,
            file_name: raw.file_name,
            export_relative_path: rel,
            file_size: raw.file_size,
            mime_type: raw.mime_type,
            etag: raw.etag,
            hash: raw.hash,
            is_main: raw.is_main,
            is_dirty: raw.is_dirty,
            version: raw.version,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
        });
    }
    let _ = attachments_root;

    // Annotations: only those attached to exported attachments; normalize document_id
    for raw in snap.annotations_raw {
        match normalize_document_id(&raw.document_id, &exported_att_ids) {
            Some(att_id) => {
                bundle.annotations.push(AnnotationExportRow {
                    id: raw.id,
                    document_id: att_id,
                    page: raw.page,
                    kind: raw.kind,
                    color: raw.color,
                    range: raw.range,
                    note: raw.note,
                    created_at: raw.created_at,
                    updated_at: raw.updated_at,
                    version: raw.version,
                });
            }
            None => {
                counts.annotations_skipped_orphan += 1;
            }
        }
    }

    bundle.literature_notes = snap
        .literature_notes
        .into_iter()
        .filter(|n| lit_ids.contains(&n.literature_id))
        .collect();

    bundle.citations = snap
        .citations
        .into_iter()
        .filter(|c| lit_ids.contains(&c.source_id) && lit_ids.contains(&c.target_id))
        .collect();

    bundle.feeds = snap.feeds;
    bundle.feed_items = snap.feed_items;

    counts.literatures = bundle.literatures.len();
    counts.authors = bundle.authors.len();
    counts.publications = bundle.publications.len();
    counts.tags = bundle.tags.len();
    counts.folders = bundle.folders.len();
    counts.attachments = bundle.attachments.len();
    counts.annotations = bundle.annotations.len();
    counts.literature_notes = bundle.literature_notes.len();
    counts.citations = bundle.citations.len();
    counts.feeds = bundle.feeds.len();
    counts.feed_items = bundle.feed_items.len();

    Ok((bundle, counts))
}

fn write_bundle_files(
    package_root: &Path,
    bundle: &LibraryExportBundle,
    counts: ExportCounts,
    package_root_name: &str,
) -> Result<()> {
    fs::create_dir_all(package_root)?;
    fs::create_dir_all(package_root.join("library"))?;
    fs::create_dir_all(package_root.join("attachments"))?;

    let lib = package_root.join("library");
    write_json(&lib.join("literatures.json"), &bundle.literatures)?;
    write_json(&lib.join("publications.json"), &bundle.publications)?;
    write_json(&lib.join("authors.json"), &bundle.authors)?;
    write_json(
        &lib.join("literature_authors.json"),
        &bundle.literature_authors,
    )?;
    write_json(&lib.join("folders.json"), &bundle.folders)?;
    write_json(
        &lib.join("literature_folders.json"),
        &bundle.literature_folders,
    )?;
    write_json(&lib.join("tags.json"), &bundle.tags)?;
    write_json(&lib.join("literature_tags.json"), &bundle.literature_tags)?;
    write_json(&lib.join("attachments.json"), &bundle.attachments)?;
    write_json(&lib.join("annotations.json"), &bundle.annotations)?;
    write_json(&lib.join("literature_notes.json"), &bundle.literature_notes)?;
    write_json(&lib.join("citations.json"), &bundle.citations)?;
    write_json(&lib.join("feeds.json"), &bundle.feeds)?;
    write_json(&lib.join("feed_items.json"), &bundle.feed_items)?;

    let manifest = models::library_export::ExportManifest::new(
        chrono::Utc::now().timestamp(),
        env!("CARGO_PKG_VERSION"),
        package_root_name,
        counts,
    );
    write_json(&package_root.join("MANIFEST.json"), &manifest)?;
    Ok(())
}

/// 在父目录下生成不冲突的包根名：`Lumen-Library-YYYYMMDD-HHMMSS`（冲突则 `-1`…）
fn allocate_package_root(parent: &Path) -> Result<(PathBuf, String)> {
    let base = format!(
        "Lumen-Library-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let mut name = base.clone();
    let mut path = parent.join(&name);
    let mut n = 1u32;
    while path.exists() {
        name = format!("{base}-{n}");
        path = parent.join(&name);
        n += 1;
        if n > 10_000 {
            return Err(anyhow!("无法为导出包分配唯一目录名"));
        }
    }
    Ok((path, name))
}

/// 导出整个有效文献库。
///
/// `parent_dir` 为用户选择的**父目录**；实际包写入
/// `parent_dir/Lumen-Library-<timestamp>/`（含 MANIFEST、library、attachments）。
///
/// - 仅 `is_deleted = 0`
/// - 附件仅导出磁盘上存在的文件，按 `attachments/<literature_id>/<file_name>` 复制
/// - 批注仅导出挂在已导出附件上的记录，`document_id` 规范为 attachment id
pub fn export_library(
    db: &Database,
    attachments_dir: &Path,
    parent_dir: &Path,
) -> Result<LibraryExportReport> {
    info!(
        "导出: 开始全库备份 (format_version={LIBRARY_EXPORT_FORMAT_VERSION}) parent={}",
        parent_dir.display()
    );
    if parent_dir.as_os_str().is_empty() {
        return Err(anyhow!("导出目录不能为空"));
    }

    fs::create_dir_all(parent_dir).context("create export parent directory")?;
    let (package_root, package_root_name) = allocate_package_root(parent_dir)?;
    fs::create_dir_all(&package_root).context("create export package root")?;

    info!("导出: 包根 package_root={}", package_root.display());

    let snap = db.export_snapshot().context("read library snapshot")?;
    let (bundle, counts) = filter_bundle(snap, attachments_dir, &package_root)?;
    write_bundle_files(&package_root, &bundle, counts.clone(), &package_root_name)
        .context("write export package")?;

    info!(
        "导出: 完成 package_root={package_root_name} lits={} atts={} skipped_att={} anns={} skipped_ann={}",
        counts.literatures,
        counts.attachments,
        counts.attachments_skipped_missing_file,
        counts.annotations,
        counts.annotations_skipped_orphan
    );

    Ok(LibraryExportReport {
        dest_dir: package_root.to_string_lossy().to_string(),
        counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::constructors::{create_attachment, create_literature};
    use models::library_export::{AnnotationExportRow, AttachmentExportRow};
    use models::{AnnotationColor, AnnotationKind, LiteratureType};

    #[test]
    fn normalize_document_id_variants() {
        let mut exported = HashSet::new();
        exported.insert("att-1".to_string());
        assert_eq!(
            normalize_document_id("att-1", &exported).as_deref(),
            Some("att-1")
        );
        assert_eq!(
            normalize_document_id("lit-9::att-1", &exported).as_deref(),
            Some("att-1")
        );
        assert_eq!(normalize_document_id("att-missing", &exported), None);
        assert_eq!(normalize_document_id("lit-9", &exported), None);
    }

    #[test]
    fn export_skips_missing_files_and_orphan_annotations() {
        let dir = std::env::temp_dir().join(format!("lumen-export-test-{}", uuid::Uuid::new_v4()));
        let att_dir = dir.join("library-attachments");
        let out = dir.join("out");
        fs::create_dir_all(&att_dir).unwrap();

        let db = Database::new(":memory:").unwrap();
        let lit = create_literature("lit-1", "Paper", LiteratureType::Article);
        db.insert_literature(&lit).unwrap();

        let mut missing = create_attachment(
            "att-missing".to_string(),
            "lit-1".to_string(),
            dir.join("nope.pdf").to_string_lossy().to_string(),
            "nope.pdf".to_string(),
            1,
        );
        missing.is_main = true;
        db.insert_attachment(&missing).unwrap();

        let present_src = att_dir.join("ok.pdf");
        fs::write(&present_src, b"%PDF-1.4 test").unwrap();
        let mut present = create_attachment(
            "att-ok".to_string(),
            "lit-1".to_string(),
            present_src.to_string_lossy().to_string(),
            "ok.pdf".to_string(),
            13,
        );
        present.is_main = false;
        db.insert_attachment(&present).unwrap();

        let orphan = models::Annotation {
            id: "ann-orphan".into(),
            document_id: "att-missing".into(),
            page: 1,
            kind: AnnotationKind::Highlight,
            color: AnnotationColor::Yellow,
            range: None,
            note: None,
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_dirty: false,
        };
        db.save_annotation(&orphan).unwrap();
        let keep = models::Annotation {
            id: "ann-ok".into(),
            document_id: "lit-1::att-ok".into(),
            page: 1,
            kind: AnnotationKind::Underline,
            color: AnnotationColor::Blue,
            range: None,
            note: Some("x".into()),
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_dirty: false,
        };
        db.save_annotation(&keep).unwrap();

        let report = export_library(&db, &att_dir, &out).unwrap();
        assert_eq!(report.counts.attachments, 1);
        assert_eq!(report.counts.attachments_skipped_missing_file, 1);
        assert_eq!(report.counts.annotations, 1);
        assert_eq!(report.counts.annotations_skipped_orphan, 1);

        let pkg = PathBuf::from(&report.dest_dir);
        assert!(
            pkg.starts_with(&out),
            "包根应在所选父目录下: {} vs {}",
            pkg.display(),
            out.display()
        );
        assert!(
            pkg.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("Lumen-Library-")),
            "包根名应以 Lumen-Library- 开头: {}",
            pkg.display()
        );
        // 父目录不应直接出现 MANIFEST
        assert!(!out.join("MANIFEST.json").exists());
        assert!(pkg.join("MANIFEST.json").is_file());

        let att_json = fs::read_to_string(pkg.join("library/attachments.json")).unwrap();
        let atts: Vec<AttachmentExportRow> = serde_json::from_str(&att_json).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].id, "att-ok");
        assert_eq!(atts[0].file_name, "ok.pdf");
        assert!(pkg.join("attachments/lit-1/ok.pdf").is_file());

        let ann_json = fs::read_to_string(pkg.join("library/annotations.json")).unwrap();
        let anns: Vec<AnnotationExportRow> = serde_json::from_str(&ann_json).unwrap();
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].document_id, "att-ok");

        let man = fs::read_to_string(pkg.join("MANIFEST.json")).unwrap();
        assert!(man.contains("lumen-library-export"));
        assert!(man.contains("\"import_supported\": false"));
        assert!(man.contains("package_root"));
        assert!(man.contains("Lumen-Library-"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_writes_soft_delete_free_literatures() {
        let dir = std::env::temp_dir().join(format!("lumen-export-del-{}", uuid::Uuid::new_v4()));
        let out = dir.join("out");
        let db = Database::new(":memory:").unwrap();
        let live = create_literature("lit-live", "Keep", LiteratureType::Article);
        db.insert_literature(&live).unwrap();
        let dead = create_literature("lit-dead", "Drop", LiteratureType::Book);
        db.insert_literature(&dead).unwrap();
        db.delete_literature("lit-dead").unwrap();

        let report = export_library(&db, &dir.join("atts"), &out).unwrap();
        assert_eq!(report.counts.literatures, 1);
        let pkg = PathBuf::from(&report.dest_dir);
        let text = fs::read_to_string(pkg.join("library/literatures.json")).unwrap();
        assert!(text.contains("lit-live"));
        assert!(!text.contains("lit-dead"));
        let _ = fs::remove_dir_all(&dir);
    }
}
