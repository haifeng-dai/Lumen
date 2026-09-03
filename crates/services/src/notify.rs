//! 跨线程通知消息 —— 桥接 tokio 上下文 → GPUI 主循环（DataStore 刷新）。
//!
//! 纯数据枚举，不依赖 `gpui`，故置于服务层 crate 供 `app`（组合根）与
//! 各服务模块构造通知闭包时使用；lumen 侧的 `DataStore`（GPUI `Entity`）
//! 订阅同名广播通道并据此刷新。

use crate::sync::attachments::{AttachmentSyncIssue, PrepareAttachmentErrorKind};

/// 附件统一打开流程的上行纯数据通知。
///
/// 只携带脱敏枚举，不携带 attachment ID、路径、文件名、object key、
/// 版本、hash 或后端凭据；UI 层据此映射 Toast 与语言文案。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentOpenNotice {
    /// 准备成功但存在只读冲突/分歧/待下载诊断（不阻止打开）
    Issue(AttachmentSyncIssue),
    /// 准备失败（类型化、脱敏），不得回退到数据库旧路径打开
    PrepareFailed(PrepareAttachmentErrorKind),
    /// 已准备成功、但系统/外部程序打开失败（与准备失败语义分离）
    OpenFailed,
}

/// service 层在 tokio 中写 DB 后，无法直接调用 `Entity::update`，
/// 只能通过广播此消息让 GPUI 主循环完成 UI 刷新。
#[derive(Clone, Debug)]
pub enum RefreshMsg {
    /// 领域数据变更（触发 DataStore.refresh_from_db）
    DataChanged,
    /// UI 状态变更（仅触发 cx.notify，无需刷新 DB）
    UiChanged,
    /// 附件打开流程的诊断/错误上行（由 `AttachmentOpenNotice` 携带脱敏枚举）
    AttachmentOpenNotice(AttachmentOpenNotice),
}
