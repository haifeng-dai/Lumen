use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// 远端文件资料库标识
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileLibraryIdentity {
    pub protocol_version: u32,
    pub file_library_id: String,
    pub database_library_id: String,
    pub created_at: i64,
}

/// 严格解析并校验 FileLibraryIdentity JSON 字符串。
///
/// 要求：
/// - JSON 必须为对象，正好包含 protocol_version, file_library_id, database_library_id, created_at 四个字段；
/// - 不允许任何未知字段；
/// - protocol_version 必须为 1；
/// - file_library_id 与 database_library_id 必须为规范小写连字符 UUID；
/// - created_at 必须为 JSON 整数。
pub fn parse_file_library_identity_json(json_str: &str) -> anyhow::Result<FileLibraryIdentity> {
    let identity: FileLibraryIdentity = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("invalid file library identity JSON format: {e}"))?;

    if identity.protocol_version != 1 {
        return Err(anyhow::anyhow!(
            "unsupported protocol version {}, expected 1",
            identity.protocol_version
        ));
    }

    validate_canonical_uuid(&identity.file_library_id, "file_library_id")?;
    validate_canonical_uuid(&identity.database_library_id, "database_library_id")?;

    Ok(identity)
}

/// 校验字符串是否为规范小写 UUID
pub fn validate_canonical_uuid(s: &str, field_name: &str) -> anyhow::Result<()> {
    if s.is_empty()
        || s.contains('/')
        || s.contains('\\')
        || s.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(anyhow::anyhow!(
            "invalid {field_name}: contains whitespace, control or path characters"
        ));
    }
    let parsed = uuid::Uuid::parse_str(s)
        .map_err(|_| anyhow::anyhow!("{field_name} is not a valid UUID: {s}"))?;
    if parsed.to_string() != s {
        return Err(anyhow::anyhow!(
            "{field_name} is not in canonical lowercase UUID format: {s}"
        ));
    }
    Ok(())
}

/// 文件资料库探测结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryInspection {
    MissingEmpty,
    MissingNonEmpty,
    Present(FileLibraryIdentity),
}

/// 远端对象条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteObjectEntry {
    pub object_key: String,
    pub remote_version: String,
}

/// 对象上传结果（区分新创建与已存在）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadObjectResult {
    /// 成功创建新对象，携带远端版本（ETag 或 Drive modifiedTime/id）
    Created(String),
    /// 远端对象已存在（create-only 保护生效）
    AlreadyExists,
}

/// 严格校验对象键是否为规范的 objects/v1/<canonical UUID>
pub fn validate_canonical_object_key(object_key: &str) -> Result<String> {
    let prefix = "objects/v1/";
    if !object_key.starts_with(prefix) {
        return Err(anyhow!("对象键 '{object_key}' 缺少规范前缀 '{prefix}'"));
    }
    let leaf = &object_key[prefix.len()..];
    validate_canonical_uuid(leaf, "object key UUID")?;
    Ok(leaf.to_string())
}

/// 附件同步后端接口（新协议）
///
/// 每个实现（WebDAV、SFTP、Google Drive 等）
/// 负责将协议细节封闭在文件内部。
pub trait AttachmentBackend: Send + Sync {
    /// 后端名称标识，如 "webdav"、"google_drive"
    fn name(&self) -> &str;

    /// 是否已启用
    fn is_enabled(&self) -> bool;

    /// 测试连接是否可用
    fn test_connection(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    /// 探测文件资料库身份
    fn inspect_library(&self) -> Pin<Box<dyn Future<Output = Result<LibraryInspection>> + Send>>;

    /// 初始化文件资料库（创建 identity 与 objects 目录）
    fn initialize_library(
        &self,
        identity: FileLibraryIdentity,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    /// 列出所有对象
    fn list_objects(&self) -> Pin<Box<dyn Future<Output = Result<Vec<RemoteObjectEntry>>> + Send>>;

    /// 条件上传对象（仅当对象不存在时创建，绝不覆盖）
    fn upload_object_if_absent(
        &self,
        object_key: String,
        local_path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<UploadObjectResult>> + Send>>;

    /// 下载对象到临时文件路径（404 返回 None，成功返回远端版本）
    fn download_object(
        &self,
        object_key: String,
        temporary_path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>>;

    /// 删除对象（404 视为成功已删除）
    fn delete_object(&self, object_key: String)
    -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    /// 返回该 backend 实例的不可逆配置指纹（SHA-256 十六进制字符串）。
    ///
    /// 规则：
    /// - 输入为规范化非秘密标识：backend 种类 + endpoint/path/账户稳定标识；
    /// - 密码、access token、refresh token、client secret 永不参与摘要；
    /// - 相同规范化配置必须输出相同指纹；
    /// - endpoint/path/账户变化必须改变指纹。
    fn configuration_fingerprint(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_library_identity_accepts_valid_v1_shape() {
        let valid_json = r#"{
            "protocol_version": 1,
            "file_library_id": "550e8400-e29b-41d4-a716-446655440000",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "created_at": 1700000000
        }"#;
        let identity = parse_file_library_identity_json(valid_json).unwrap();
        assert_eq!(identity.protocol_version, 1);
        assert_eq!(
            identity.file_library_id,
            "550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(
            identity.database_library_id,
            "6ba7b810-9dad-11d1-80b4-00c04fd430c8"
        );
        assert_eq!(identity.created_at, 1700000000);
    }

    #[test]
    fn file_library_identity_rejects_missing_fields() {
        // Missing created_at
        let json = r#"{
            "protocol_version": 1,
            "file_library_id": "550e8400-e29b-41d4-a716-446655440000",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8"
        }"#;
        assert!(parse_file_library_identity_json(json).is_err());
    }

    #[test]
    fn file_library_identity_rejects_unknown_fields() {
        let json = r#"{
            "protocol_version": 1,
            "file_library_id": "550e8400-e29b-41d4-a716-446655440000",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "created_at": 1700000000,
            "extra_field": "disallowed"
        }"#;
        assert!(parse_file_library_identity_json(json).is_err());
    }

    #[test]
    fn file_library_identity_rejects_unsupported_version() {
        let json = r#"{
            "protocol_version": 2,
            "file_library_id": "550e8400-e29b-41d4-a716-446655440000",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "created_at": 1700000000
        }"#;
        assert!(parse_file_library_identity_json(json).is_err());
    }

    #[test]
    fn file_library_identity_rejects_non_canonical_uuid() {
        // Uppercase
        let json_upper = r#"{
            "protocol_version": 1,
            "file_library_id": "550E8400-E29B-41D4-A716-446655440000",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "created_at": 1700000000
        }"#;
        assert!(parse_file_library_identity_json(json_upper).is_err());

        // Non-UUID string
        let json_invalid = r#"{
            "protocol_version": 1,
            "file_library_id": "not-a-uuid",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "created_at": 1700000000
        }"#;
        assert!(parse_file_library_identity_json(json_invalid).is_err());

        // With whitespace
        let json_ws = r#"{
            "protocol_version": 1,
            "file_library_id": " 550e8400-e29b-41d4-a716-446655440000 ",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "created_at": 1700000000
        }"#;
        assert!(parse_file_library_identity_json(json_ws).is_err());
    }

    #[test]
    fn file_library_identity_rejects_invalid_types() {
        // created_at as string
        let json_str_time = r#"{
            "protocol_version": 1,
            "file_library_id": "550e8400-e29b-41d4-a716-446655440000",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "created_at": "1700000000"
        }"#;
        assert!(parse_file_library_identity_json(json_str_time).is_err());

        // created_at as float
        let json_float_time = r#"{
            "protocol_version": 1,
            "file_library_id": "550e8400-e29b-41d4-a716-446655440000",
            "database_library_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "created_at": 1700000000.5
        }"#;
        assert!(parse_file_library_identity_json(json_float_time).is_err());
    }

    #[test]
    fn file_library_identity_and_remote_object_entry_have_clean_structures() {
        let identity = FileLibraryIdentity {
            protocol_version: 1,
            file_library_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            database_library_id: "6ba7b810-9dad-11d1-80b4-00c04fd430c8".to_string(),
            created_at: 1700000000,
        };
        let json = serde_json::to_string(&identity).unwrap();
        let decoded: FileLibraryIdentity = parse_file_library_identity_json(&json).unwrap();
        assert_eq!(identity, decoded);

        let entry = RemoteObjectEntry {
            object_key: "objects/v1/a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".to_string(),
            remote_version: "etag-abc".to_string(),
        };
        assert_eq!(
            entry.object_key,
            "objects/v1/a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"
        );
    }

    #[test]
    fn validate_canonical_object_key_validations() {
        // Valid
        let valid_key = "objects/v1/550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            validate_canonical_object_key(valid_key).unwrap(),
            "550e8400-e29b-41d4-a716-446655440000"
        );

        // Missing prefix
        assert!(validate_canonical_object_key("550e8400-e29b-41d4-a716-446655440000").is_err());
        assert!(
            validate_canonical_object_key("objects/v2/550e8400-e29b-41d4-a716-446655440000")
                .is_err()
        );

        // Display name or non-UUID
        assert!(validate_canonical_object_key("objects/v1/paper.pdf").is_err());
        assert!(validate_canonical_object_key("objects/v1/../secret.txt").is_err());
        assert!(
            validate_canonical_object_key("objects/v1/550E8400-E29B-41D4-A716-446655440000")
                .is_err()
        );
    }
}
