use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter, Result};

#[cfg(feature = "gpui")]
use gpui::SharedString;
#[cfg(feature = "gpui")]
use gpui_component::select::SelectItem;

mod de;
mod en_us;
mod es;
mod fr;
mod ja;
mod ko;
pub mod literature_type;
mod ru;
mod zh_cn;
mod zh_tw;

pub use literature_type::LiteratureTypeExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    ZhCn,
    ZhTw,
    En,
    Ja,
    Ko,
    Ru,
    Fr,
    De,
    Es,
}

impl Language {
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::ZhCn => "简体中文",
            Self::ZhTw => "繁體中文",
            Self::En => "English",
            Self::Ja => "日本語",
            Self::Ko => "한국어",
            Self::Ru => "Русский",
            Self::Fr => "Français",
            Self::De => "Deutsch",
            Self::Es => "Español",
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::ZhTw => "zh-TW",
            Self::En => "en-US",
            Self::Ja => "ja-JP",
            Self::Ko => "ko-KR",
            Self::Ru => "ru-RU",
            Self::Fr => "fr-FR",
            Self::De => "de-DE",
            Self::Es => "es-ES",
        }
    }
}

impl std::str::FromStr for Language {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "zh-TW" | "zh-HK" => Ok(Self::ZhTw),
            "en-US" | "en" => Ok(Self::En),
            "ja-JP" | "ja" => Ok(Self::Ja),
            "ko-KR" | "ko" => Ok(Self::Ko),
            "ru-RU" | "ru" => Ok(Self::Ru),
            "fr-FR" | "fr" => Ok(Self::Fr),
            "de-DE" | "de" => Ok(Self::De),
            "es-ES" | "es" => Ok(Self::Es),
            "zh-CN" | "zh" => Ok(Self::ZhCn),
            _ => Err(()),
        }
    }
}

#[cfg(feature = "gpui")]
impl SelectItem for Language {
    type Value = Language;

    fn title(&self) -> SharedString {
        self.name().into()
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

impl Display for Language {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.name())
    }
}

pub trait Translatable {
    fn translate(&self, lang: Language) -> &'static str;
}

#[derive(Debug, Clone, Copy)]
pub enum I18nKey {
    // Tab
    Library,
    Subscription,

    // Native macOS Menu（顶栏系统菜单，随 app 语言本地化）
    Hide,
    HideOthers,
    ShowAll,
    Services,

    // Folder Panel
    AllLiterature,
    Uncategorized,
    Trash,
    Tags,
    AllSubscription,
    Unread,
    StatusReading,
    StatusRead,
    FolderNamePlaceholder,
    TagNamePlaceholder,
    SearchOrCreateTags,
    CreateTag,
    Version,
    EmptyFolder,
    NoMatchFound,

    // Toolbar
    SearchBoxPlaceholder,
    ManualAdd,
    BibTeXImport,
    DoiImport,
    ArXivImport,
    DblpSearch,
    DuplicateGroups,
    SyncConflicts,
    NoDuplicatesFound,
    DuplicateSearch,

    // Metadata Selector
    SelectMetadataCandidate,

    // Context Menu
    NewFolder,
    EmptyTrash,
    Rename,
    Delete,
    NewSubscription,
    OpenInBrowser,
    MarkAsRead,
    MarkAsUnread,
    UpdateSubscription,
    EditSubscription,
    Unsubscribe,
    AddSubscription,
    UpdateAllSubscriptions,
    SubscriptionUpdated,
    SubscriptionUpdateFailed,
    NewSubFolder,
    Edit,
    Quit,
    PermanentDelete,
    CopyCitation,
    FetchFrom,
    BatchFetchMetadata,
    AddTo,
    RestoreTo,
    RemoveFromFolder,
    RevealInFinder,
    RevealInExplorer,
    OpenPath,
    ReplaceFile,
    DeleteFile,
    SelectNewFile,
    Export,
    ExportAnnotatedPdf,
    ExportAnnotatedPdfSuccess,
    ExportAnnotatedPdfFailed,
    ExportAnnotatedPdfNoAnnotations,
    Confirm,
    LoadingMetadata,
    FetchFailed,
    Retry,
    Close,
    ConfirmFetch,
    FetchFromSource,
    FetchPlaceholderDoi,
    FetchPlaceholderArxiv,
    FetchPlaceholderBibtex,
    FetchPlaceholderDblp,
    FetchPlaceholderOpenAlex,
    NoContentOrInvalidFormat,
    ImportFailed,

    // Literature Editor
    LiteratureEditor,
    AuthorPlaceholder,
    JournalPlaceholder,
    Month,
    Day,
    Publisher,
    // Literature Compare
    Field,
    LocalData,
    RemoteData,
    // Subscription Editor
    SubscriptionEditor,
    FeedName,
    FeedUrl,
    UpdateInterval,
    SubscriptionNamePlaceholder,
    SubscriptionUrlPlaceholder,
    UpdateIntervalPlaceholder,
    Add,

    // Subscription Detail
    SelectedSubscriptionCount,
    AddToLibrary,
    UpdatedAt,
    NoSubscriptionSelected,
    NoAbstract,

    // Citation Popup
    CopyCitationTitle,
    Style,
    Preview,
    NoLiteratureSelectedForCitation,
    CitationError,

    // Citation Copy Submenu (replaces popup)
    CitationBibTeX,
    CitationIeee,
    CopiedToClipboard,

    // Citation Settings
    CitationSettings,
    AbbreviateJournalInCitation,

    // Literature Types
    Type,
    TypeArticle,
    TypeBook,
    TypeConference,
    TypeThesis,
    TypePreprint,
    TypeTechnicalReport,
    TypeWebpage,
    TypeOther,

    // Folder Types
    // Literature Detail
    Folders,
    Title,
    Authors,
    Journal,
    Year,
    Volume,
    Issue,
    Pages,
    Url,
    Doi,
    ArXiv,
    Abstract,
    Notes,
    Attachments,
    NoLiteratureSelected,
    SelectedCount,
    MainFile,
    Attachment,
    SetAsMainFile,
    SetAsAttachment,
    Expand,
    Collapse,
    Publication,
    PublicationAbbreviation,
    RelatedLiterature,
    AddCitation,
    References,
    CitedBy,

    // Settings
    Settings,
    Language,
    Appearance,
    UiScale,
    LogLevel,
    NotificationLevel,
    ThemeStyle,
    Theme,
    Dark,
    Light,
    System,
    General,
    Sync,
    About,
    Cancel,
    Save,
    LibrarySettings,
    AttachmentDir,
    AttachmentDirDesc,
    DatabaseDir,
    DatabaseDirDesc,
    FilenameTemplate,
    FilenameTemplateDesc,
    BatchRename,
    CleanupOrphanedFiles,
    GeneralOptions,
    CloudSyncDesc,
    AboutDesc,
    Copyright,

    // Advanced Filter
    // Sort
    SortBy,
    SortByTitle,
    SortByAuthor,
    SortByYear,
    SortByJournal,
    SortAscending,
    SortDescending,

    // Sync
    SyncMetadata,
    SyncAttachments,
    TestConnection,
    WebDavSettings,
    EnableWebDav,
    DatabaseSettings,
    EndpointUrl,
    Username,
    Password,
    RemotePath,
    Host,
    Port,
    DatabaseName,
    EnableSSL,
    UseRemoteDatabase,
    ConnectionSuccess,
    ConnectionFailed,

    SyncMetadataTab,
    SyncAttachmentTab,
    GoogleDriveDesc,
    EnableGoogleDrive,
    ClientId,
    ClientSecret,
    Authorize,
    DataManagement,
    ClearLocalDb,
    ClearLocalFiles,
    ClearCloudDb,
    ClearCloudFiles,
    PurgeSyncedDeletions,

    // PDF Viewer
    PdfViewerSettings,
    PdfViewerSettingsDesc,
    UseCustomPdfViewer,
    PdfViewerPathMacos,
    PdfViewerPathWindows,
    // Network Proxy
    NetworkProxySettings,
    EnableProxyServer,
    ProxyAddress,
    ProxyDesc,

    // Service Errors
    // Error/Notification
    FileNotFoundTitle,
    FileNotFoundMsg,
    DataConsistentTitle,
    DataConsistentMsg,
    LiteratureMergedTitle,
    LiteratureMergedMsg,

    // Fetch Error Tips
    FetchFailedArxiv,
    FetchFailedDblp,
    FetchFailedCrossref,
    FetchFailedOpenAlex,

    // Batch Update
    BatchUpdatingMetadata,

    // Settings - Translation
    TranslationSettings,
    TranslationSettingsDesc,
    TranslationEngine,
    TranslationSettingsTab,
    NoApiKeyRequired,
    AiBackend,
    NiuTransApiKey,
    GoogleApiKey,
    BaiduApiKey,
    YoudaoApiKey,
    DeepLApiKey,
    TargetLanguage,
    EngineGoogleFree,
    EngineBingFree,
    EngineGoogleCloud,
    EngineNiuTrans,
    EngineBaidu,
    EngineYoudao,
    EngineDeeplFree,
    EngineDeeplPro,
    EngineAi,
    AiApiKey,
    AiApiBase,
    AiModel,
    AiContextWindow,
    AiCompressionStrategy,
    SlidingWindow,
    SummaryCompression,
    AiBackendName,
    AiBackendType,
    AiAddBackend,
    AiActive,
    AiNoBackends,
    InternalReaderDesc,
    SelectMacosPdfReader,
    SelectWindowsPdfReader,

    // PDF View - Notes
    EditNotesMarkdown,

    // Feed
    // Bookmark
    UnnamedBookmark,

    // Pdf Viewer
    NotePlaceholder,
    ViewNote,
    AddNote,
    Highlight,
    Underline,
    LoadingOutline,
    NoOutline,
    RectangleAnnotation,
    PageRange,
    SinglePage,
    SelectTextToTranslate,
    Translate,
    OriginalSection,
    TranslationSection,
    Translating,
    TranslationPending,
    NoNotes,
    CopyAsImage,
    PdfEngineError,
    CloseWindow,
    TranslationNotImplemented,
    CreatePip,
    DeletePage,
    SaveAsImage,

    // Pdf Viewer - AI Chat
    Chat,
    NewChat,
    ChatInputPlaceholder,
    NoChatSessions,
    DeleteChat,
    SendSelection,
    QuoteLabel,
    AttachFile,
    NoAttachments,
    AiThinking,
    BackToSessions,
    EditSystemPrompt,
    DefaultChatTitle,
    ChatSessionDeleted,

    // Settings - AI Backends / Chat
    AiBackendsSettingsTab,
    AiChatSettingsTab,
    DefaultSystemPrompt,
    BackendSelection,
    ActiveBackend,
    EnableThinking,

    // PDF Search
    SearchInPdf,
    SearchInputPlaceholder,
}

impl Translatable for I18nKey {
    fn translate(&self, lang: Language) -> &'static str {
        match lang {
            Language::ZhCn => zh_cn::translate(*self),
            Language::ZhTw => zh_tw::translate(*self),
            Language::En => en_us::translate(*self),
            Language::Ja => ja::translate(*self),
            Language::Ko => ko::translate(*self),
            Language::Ru => ru::translate(*self),
            Language::Fr => fr::translate(*self),
            Language::De => de::translate(*self),
            Language::Es => es::translate(*self),
        }
    }
}

#[must_use]
pub fn t(key: I18nKey, lang: Language) -> &'static str {
    key.translate(lang)
}

#[must_use]
pub fn tf(key: I18nKey, lang: Language, args: &[&str]) -> String {
    let mut s = t(key, lang).to_string();
    for arg in args {
        s = s.replacen("{}", arg, 1);
    }
    s
}
