use crate::I18nKey;

pub fn translate(key: I18nKey) -> &'static str {
    match key {
        I18nKey::Library => "Library",
        I18nKey::Subscription => "Subscription",
        I18nKey::AllLiterature => "All Literatures",
        I18nKey::Uncategorized => "Uncategorized",
        I18nKey::Trash => "Trash",
        I18nKey::Tags => "Tags",
        I18nKey::AllSubscription => "All Subscriptions",
        I18nKey::Unread => "Unread",
        I18nKey::StatusReading => "Reading",
        I18nKey::StatusRead => "Read",
        I18nKey::FolderNamePlaceholder => "Folder Name",
        I18nKey::TagNamePlaceholder => "Tag Name",
        I18nKey::SearchOrCreateTags => "Search or create tags...",
        I18nKey::CreateTag => "Create \"{}\"",
        I18nKey::Version => "Version",
        I18nKey::EmptyFolder => "Folder is empty",
        I18nKey::NoMatchFound => "No matching literatures found",
        I18nKey::SearchBoxPlaceholder => "Search literatures, authors or journals...",
        I18nKey::ManualAdd => "Manual Add",
        I18nKey::BibTeXImport => "BibTeX Import",
        I18nKey::DoiImport => "DOI Import",
        I18nKey::ArXivImport => "ArXiv Import",
        I18nKey::DblpSearch => "DBLP Search",
        I18nKey::DuplicateGroups => "Duplicate Groups",
        I18nKey::SyncConflicts => "Version Conflicts",
        I18nKey::NoDuplicatesFound => "No duplicates found",
        I18nKey::DuplicateSearch => "Find Duplicates",
        I18nKey::NewFolder => "New Folder",
        I18nKey::EmptyTrash => "Empty Trash",
        I18nKey::Rename => "Rename",
        I18nKey::Delete => "Delete",
        I18nKey::NewSubscription => "New Subscription",
        I18nKey::OpenInBrowser => "Open in Browser",
        I18nKey::MarkAsRead => "Mark as Read",
        I18nKey::MarkAsUnread => "Mark as Unread",
        I18nKey::UpdateSubscription => "Update",
        I18nKey::EditSubscription => "Modify",
        I18nKey::Unsubscribe => "Delete",
        I18nKey::AddSubscription => "Add Subscription",
        I18nKey::UpdateAllSubscriptions => "Update All Subscriptions",
        I18nKey::SubscriptionUpdated => "Subscription {} updated",
        I18nKey::SubscriptionUpdateFailed => "Subscription {} update failed: {}",
        I18nKey::NewSubFolder => "New Sub-folder",
        I18nKey::Edit => "Edit",
        I18nKey::Quit => "Quit",
        I18nKey::PermanentDelete => "Permanent Delete",
        I18nKey::CopyCitation => "Citation",
        I18nKey::FetchFrom => "Fetch from...",
        I18nKey::BatchFetchMetadata => "Batch Update Metadata",
        I18nKey::AddTo => "Add to...",
        I18nKey::RestoreTo => "Restore to...",
        I18nKey::RemoveFromFolder => "Remove from Folder",
        I18nKey::RevealInFinder => "Reveal in Finder",
        I18nKey::RevealInExplorer => "Reveal in Explorer",
        I18nKey::OpenPath => "Open Path",
        I18nKey::ReplaceFile => "Replace File",
        I18nKey::DeleteFile => "Delete Permanently",
        I18nKey::Export => "Export",
        I18nKey::ExportAnnotatedPdf => "Export PDF with Annotations",
        I18nKey::ExportAnnotatedPdfSuccess => "PDF exported (with annotations)",
        I18nKey::ExportAnnotatedPdfFailed => "Failed to export annotated PDF",
        I18nKey::ExportAnnotatedPdfNoAnnotations => "This document has no annotations to export",
        I18nKey::SelectNewFile => "Select New File",
        I18nKey::Confirm => "OK",
        I18nKey::LoadingMetadata => "Fetching metadata from remote source...",
        I18nKey::FetchFailed => "Fetch Failed",
        I18nKey::Retry => "Retry",
        I18nKey::Close => "Close",
        I18nKey::ConfirmFetch => "Fetch Now",
        I18nKey::FetchFromSource => "Fetch from {}",
        I18nKey::FetchPlaceholderDoi => "Please enter DOI (e.g. 10.1000/xyz123)",
        I18nKey::FetchPlaceholderArxiv => "Please enter ArXiv ID (e.g. 2101.12345)",
        I18nKey::FetchPlaceholderBibtex => "Please paste BibTeX content",
        I18nKey::FetchPlaceholderDblp => "Enter title or keywords to search DBLP",
        I18nKey::FetchPlaceholderOpenAlex => "Enter title or keywords to search OpenAlex",
        I18nKey::NoContentOrInvalidFormat => "Content empty or invalid format",
        I18nKey::ImportFailed => "Import Failed",

        I18nKey::LiteratureEditor => "Literature Editor",
        I18nKey::AuthorPlaceholder => "Authors (comma separated)",
        I18nKey::JournalPlaceholder => "Journal / Conference / Book Title",
        I18nKey::Month => "Month",
        I18nKey::Day => "Day",
        I18nKey::Publisher => "Publisher",
        I18nKey::Field => "Field",
        I18nKey::LocalData => "Local Data",
        I18nKey::RemoteData => "Remote Data",
        I18nKey::SubscriptionEditor => "Subscription Editor",
        I18nKey::FeedName => "Name",
        I18nKey::FeedUrl => "URL",
        I18nKey::UpdateInterval => "Update Interval",
        I18nKey::SubscriptionNamePlaceholder => "Name (e.g. arXiv cs.CV)",
        I18nKey::SubscriptionUrlPlaceholder => "RSS URL",
        I18nKey::UpdateIntervalPlaceholder => "Update interval (hours)",
        I18nKey::Add => "Add",

        I18nKey::SelectedSubscriptionCount => "{} subscription items selected",
        I18nKey::AddToLibrary => "+ Add to Library",
        I18nKey::UpdatedAt => "Updated At",
        I18nKey::NoSubscriptionSelected => "No subscription selected",
        I18nKey::NoAbstract => "No abstract available",

        I18nKey::CopyCitationTitle => "Citation",
        I18nKey::Style => "Style",
        I18nKey::Preview => "Preview",
        I18nKey::NoLiteratureSelectedForCitation => "No literature selected",
        I18nKey::CitationError => "Failed to generate citation",
        I18nKey::CitationBibTeX => "BibTeX",
        I18nKey::CitationIeee => "IEEE",
        I18nKey::CopiedToClipboard => "Copied to clipboard",
        I18nKey::CitationSettings => "Citation Settings",
        I18nKey::AbbreviateJournalInCitation => "Abbreviate journal name in citations",

        I18nKey::Type => "Type",
        I18nKey::TypeArticle => "Journal Article",
        I18nKey::TypeBook => "Book",
        I18nKey::TypeConference => "Conference Paper",
        I18nKey::TypeThesis => "Thesis",
        I18nKey::TypePreprint => "Preprint",
        I18nKey::TypeTechnicalReport => "Technical Report",
        I18nKey::TypeWebpage => "Webpage",
        I18nKey::TypeOther => "Other",
        // Literature Detail
        I18nKey::Folders => "Folders",
        I18nKey::Title => "Title",
        I18nKey::Authors => "Authors",
        I18nKey::Journal => "Journal",
        I18nKey::Year => "Year",
        I18nKey::Volume => "Vol.",
        I18nKey::Issue => "Issue",
        I18nKey::Pages => "Pages",
        I18nKey::Url => "URL",
        I18nKey::Doi => "DOI",
        I18nKey::ArXiv => "ArXiv",
        I18nKey::Abstract => "Abstract",
        I18nKey::Notes => "Notes",
        I18nKey::Attachments => "Attachments",
        I18nKey::NoLiteratureSelected => "No Literature Selected",
        I18nKey::SelectedCount => "{} literatures selected",
        I18nKey::MainFile => "Main File",
        I18nKey::Attachment => "Attachment",
        I18nKey::SetAsMainFile => "Set as Main File",
        I18nKey::SetAsAttachment => "Set as Attachment",
        I18nKey::Expand => "Expand All ↓",
        I18nKey::Collapse => "Collapse ↑",
        I18nKey::Publication => "Publication",
        I18nKey::PublicationAbbreviation => "Journal Abbreviation",
        I18nKey::RelatedLiterature => "Related Literature",
        I18nKey::AddCitation => "Add Citation",
        I18nKey::References => "References",
        I18nKey::CitedBy => "Cited By",
        I18nKey::Settings => "Settings",
        I18nKey::Language => "Language",
        I18nKey::Appearance => "Appearance",
        I18nKey::UiScale => "UI Scale",
        I18nKey::LogLevel => "Log Level",
        I18nKey::NotificationLevel => "Notification Level",
        I18nKey::ThemeStyle => "Theme Style",
        I18nKey::Theme => "Theme",
        I18nKey::Dark => "Dark",
        I18nKey::Light => "Light",
        I18nKey::System => "System",
        I18nKey::General => "General",
        I18nKey::Sync => "Sync",
        I18nKey::About => "About",
        I18nKey::Cancel => "Cancel",
        I18nKey::Save => "Save",
        I18nKey::LibrarySettings => "Library Settings",
        I18nKey::AttachmentDir => "Attachment Directory",
        I18nKey::AttachmentDirDesc => {
            "All PDF literatures and attachments will be saved in this directory"
        }
        I18nKey::DatabaseDir => "Database Directory",
        I18nKey::DatabaseDirDesc => "Where database files are stored",
        I18nKey::FilenameTemplate => "Filename Format",
        I18nKey::FilenameTemplateDesc => {
            "Custom renaming rules for attachments. Available variables: {title}, {author}, {year}, {publication}, {firstname}, {lastname}, {firstchartitle}. Supports using '/' for folder hierarchy."
        }
        I18nKey::BatchRename => "Rename",
        I18nKey::BatchRenameCompleted => "Batch rename completed: {} succeeded, {} skipped",
        I18nKey::BatchRenameCompletedWithFailures => {
            "Batch rename completed: {} succeeded, {} skipped, {} failed. {}"
        }
        I18nKey::BatchRenameFailed => "Batch rename could not continue: {}",
        I18nKey::BatchRenameSourceNotRegularFile => {
            "Source file is missing or not a regular file: {}"
        }
        I18nKey::BatchRenameSourceNameUnreadable => "Source file name cannot be read",
        I18nKey::BatchRenameTargetExists => "Target file already exists: {}",
        I18nKey::BatchRenameRenameFailed => "File rename failed: {}",
        I18nKey::BatchRenameSaveFailed => {
            "Files were renamed but records were not saved; cannot continue: {}"
        }
        I18nKey::CleanupOrphanedFiles => "Cleanup",
        I18nKey::GeneralOptions => "General Options",
        I18nKey::CloudSyncDesc => {
            "Configure cloud synchronization to keep your literature metadata and attachments in sync across devices."
        }
        I18nKey::AboutDesc => {
            "A high-performance literature management application built on GPUI. Focused on a clean, smooth, and powerful academic reading and research experience."
        }
        I18nKey::Copyright => "© 2026 Lumen. All rights reserved.",
        // Sort
        I18nKey::SortBy => "Sort",
        I18nKey::SortByTitle => "Title",
        I18nKey::SortByAuthor => "Author",
        I18nKey::SortByYear => "Year",
        I18nKey::SortByJournal => "Journal",
        I18nKey::SortAscending => "Ascending",
        I18nKey::SortDescending => "Descending",

        // Sync
        I18nKey::SyncMetadata => "Sync Metadata",
        I18nKey::SyncAttachments => "Sync Attachments",
        I18nKey::FileSyncLastRun => "Last file sync",
        I18nKey::FileSyncSummaryUnavailable => "File sync summary unavailable",
        I18nKey::SyncSkippedBusy => "A sync is already running; this request was not started",
        I18nKey::SyncUploaded => "Uploaded",
        I18nKey::SyncDownloaded => "Downloaded",
        I18nKey::SyncDeleted => "Deleted",
        I18nKey::SyncFailures => "Failed",
        I18nKey::SyncWaiting => "Waiting",
        I18nKey::SyncPendingDownload => "Pending download",
        I18nKey::SyncUnknownDivergence => "Divergence",
        I18nKey::SyncUnrecoverable => "Unrecoverable",
        I18nKey::DatabaseSyncInProgress => {
            "Database sync is being upgraded; file sync remains available"
        }
        I18nKey::DatabaseSyncNeedsInitialization => "Database sync needs initialization",
        I18nKey::DatabaseSyncNeedsAdoption => "Database sync needs remote adoption",
        I18nKey::DatabaseSyncIdentityMismatch => "Database library identity mismatch",
        I18nKey::DatabaseSyncConflict => "Database sync has conflicts",
        I18nKey::DatabaseSyncPartialFailure => "Database sync partially failed",
        I18nKey::DatabaseSyncError => "Database sync error",
        I18nKey::DatabaseSyncUploaded => "Uploaded",
        I18nKey::DatabaseSyncDownloaded => "Downloaded",
        I18nKey::DatabaseSyncConflictsCount => "Conflicts",
        I18nKey::DatabaseSyncFailuresCount => "Failures",
        I18nKey::TestConnection => "Test Connection",
        I18nKey::WebDavSettings => "WebDAV Settings",
        I18nKey::EnableWebDav => "Enable WebDAV",
        I18nKey::DatabaseSettings => "Database Settings",
        I18nKey::EndpointUrl => "Endpoint URL",
        I18nKey::Username => "Username",
        I18nKey::Password => "Password",
        I18nKey::RemotePath => "Remote Path",
        I18nKey::Host => "Host",
        I18nKey::Port => "Port",
        I18nKey::DatabaseName => "Database Name",
        I18nKey::EnableSSL => "Enable SSL",
        I18nKey::UseRemoteDatabase => "Use Remote Database",
        I18nKey::ConnectionSuccess => "Connection Success",
        I18nKey::ConnectionFailed => "Connection Failed",

        I18nKey::SyncMetadataTab => "Metadata Sync",
        I18nKey::SyncAttachmentTab => "Attachment Sync",
        I18nKey::EnableGoogleDrive => "Enable Google Drive",
        I18nKey::GoogleDriveDesc => "Sync attachments to Google Drive",
        I18nKey::ClientId => "Client ID",
        I18nKey::ClientSecret => "Client Secret",
        I18nKey::Authorize => "Authorize",
        I18nKey::DataManagement => "Data Management",
        I18nKey::ClearLocalDb => "Clear Local Database",
        I18nKey::ClearLocalFiles => "Clear Local Files",
        I18nKey::CheckLocalFiles => "Check Local Files",
        I18nKey::ClearCloudDb => "Clear Cloud Database",
        I18nKey::ClearCloudFiles => "Clear Cloud Files",
        I18nKey::PurgeSyncedDeletions => "Purge Deleted Data",
        I18nKey::PurgeDeletedData => "Purge Deleted Data (Local + Remote)",

        // PDF Viewer
        I18nKey::PdfViewerSettings => "PDF Viewer Settings",
        I18nKey::PdfViewerSettingsDesc => "Customize the application for opening PDF files",
        I18nKey::UseCustomPdfViewer => "Use Custom PDF Viewer",
        I18nKey::PdfViewerPathMacos => "macOS Application",
        I18nKey::PdfViewerPathWindows => "Windows Program",
        I18nKey::SelectMetadataCandidate => "Select the best matching metadata",

        // Network Proxy
        I18nKey::NetworkProxySettings => "Network Proxy",
        I18nKey::EnableProxyServer => "Enable Custom Proxy Server",
        I18nKey::ProxyAddress => "Proxy Server Address",
        I18nKey::ProxyDesc => "Supports HTTP, HTTPS or SOCKS5 protocol, e.g. http://127.0.0.1:7890",

        // Service Errors
        // Error/Notification
        I18nKey::FileNotFoundTitle => "File Not Found",
        I18nKey::FileNotFoundMsg => "Path {:?} does not exist",
        I18nKey::DataConsistentTitle => "Data Consistent",
        I18nKey::DataConsistentMsg => {
            "The fetched metadata is identical to the local data; no merge is required."
        }
        I18nKey::LiteratureMergedTitle => "Literature Merged",
        I18nKey::LiteratureMergedMsg => {
            "The duplicate of \"{}\" is identical to the main literature and has been moved to trash."
        }

        // Fetch Error Tips
        I18nKey::FetchFailedArxiv => {
            "This literature has no ArXiv ID or related link; cannot fetch metadata from ArXiv."
        }
        I18nKey::FetchFailedDblp => "The literature title is empty; cannot search on DBLP.",
        I18nKey::FetchFailedCrossref => {
            "This literature has no DOI field or it is empty; cannot fetch metadata from Crossref."
        }
        I18nKey::FetchFailedOpenAlex => "Both DOI and title are empty; cannot search on OpenAlex.",

        // Batch Update
        I18nKey::BatchUpdatingMetadata => "Batch updating metadata ({}/{})",

        // Settings - Translation
        I18nKey::TranslationSettings => "Translation Settings",
        I18nKey::TranslationSettingsDesc => {
            "Configure translation engine and API keys for the PDF reader."
        }
        I18nKey::TranslationEngine => "Translation Engine",
        I18nKey::TranslationSettingsTab => "Translation",
        I18nKey::NoApiKeyRequired => "This engine requires no API key and can be used directly.",
        I18nKey::AiBackend => "AI Backend",
        I18nKey::NiuTransApiKey => "NiuTrans API Key",
        I18nKey::GoogleApiKey => "Google Cloud API Key",
        I18nKey::BaiduApiKey => "Baidu AppID#Key",
        I18nKey::YoudaoApiKey => "Youdao AppID#Key",
        I18nKey::DeepLApiKey => "DeepL API Key",
        I18nKey::AiApiKey => "AI API Key",
        I18nKey::AiApiBase => "API Base URL",
        I18nKey::AiModel => "Model",
        I18nKey::AiContextWindow => "Context Window",
        I18nKey::AiCompressionStrategy => "Compression Strategy",
        I18nKey::SlidingWindow => "Sliding Window",
        I18nKey::SummaryCompression => "Summary",
        I18nKey::AiBackendName => "Name",
        I18nKey::AiBackendType => "Type",
        I18nKey::AiAddBackend => "Add Backend",
        I18nKey::AiActive => "Active",
        I18nKey::AiNoBackends => "No AI backends configured",
        I18nKey::TargetLanguage => "Target Language",
        I18nKey::EngineGoogleFree => "Google (Free)",
        I18nKey::EngineBingFree => "Bing (Free)",
        I18nKey::EngineGoogleCloud => "Google Cloud",
        I18nKey::EngineNiuTrans => "NiuTrans",
        I18nKey::EngineBaidu => "Baidu",
        I18nKey::EngineYoudao => "Youdao",
        I18nKey::EngineDeeplFree => "DeepL Free",
        I18nKey::EngineDeeplPro => "DeepL Pro",
        I18nKey::EngineAi => "AI",
        I18nKey::AiBackendsSettingsTab => "AI Backends",
        I18nKey::AiChatSettingsTab => "AI Chat",
        I18nKey::DefaultSystemPrompt => "Default System Prompt",
        I18nKey::BackendSelection => "Backend Selection",
        I18nKey::ActiveBackend => "Active Backend",
        I18nKey::EnableThinking => "Thinking",
        I18nKey::InternalReaderDesc => {
            "When external reader is disabled, PDF will be opened with the built-in reader"
        }
        // PDF View - Notes
        I18nKey::EditNotesMarkdown => "Edit Notes (Markdown)",

        // Feed
        // Bookmark
        I18nKey::UnnamedBookmark => "Unnamed Bookmark",
        I18nKey::SelectMacosPdfReader => "Select macOS PDF Reader",
        I18nKey::SelectWindowsPdfReader => "Select Windows PDF Reader",

        // Pdf Viewer
        I18nKey::NotePlaceholder => "Enter note content...",
        I18nKey::ViewNote => "View Note",
        I18nKey::AddNote => "Add Note",
        I18nKey::Highlight => "Highlight",
        I18nKey::Underline => "Underline",
        I18nKey::LoadingOutline => "Loading outline...",
        I18nKey::NoOutline => "This document has no outline",
        I18nKey::RectangleAnnotation => "Rectangle",
        I18nKey::PageRange => "Page {}-{}",
        I18nKey::SinglePage => "Page {}",
        I18nKey::SelectTextToTranslate => "Select text to translate",
        I18nKey::Translate => "Translate",
        I18nKey::OriginalSection => "Original",
        I18nKey::TranslationSection => "Translation",
        I18nKey::Translating => "Translating...",
        I18nKey::TranslationPending => "Translation pending",
        I18nKey::NoNotes => "No notes yet",
        I18nKey::CopyAsImage => "Copy as Image",
        I18nKey::PdfEngineError => "PDF Render Engine Error",
        I18nKey::CloseWindow => "Close Window",
        I18nKey::TranslationNotImplemented => "Translation not implemented",
        I18nKey::CreatePip => "New PiP",
        I18nKey::DeletePage => "Delete Page",
        I18nKey::SaveAsImage => "Save as Image",

        // Pdf Viewer - AI Chat
        I18nKey::Chat => "AI Chat",
        I18nKey::NewChat => "New Chat",
        I18nKey::ChatInputPlaceholder => "Type a message...",
        I18nKey::NoChatSessions => "No conversations yet",
        I18nKey::DeleteChat => "Delete Chat",
        I18nKey::SendSelection => "Send Selection",
        I18nKey::QuoteLabel => "Quote",
        I18nKey::AttachFile => "Attach File",
        I18nKey::NoAttachments => "No attachments",
        I18nKey::AiThinking => "Thinking...",
        I18nKey::BackToSessions => "Back to Sessions",
        I18nKey::EditSystemPrompt => "Edit System Prompt",
        I18nKey::DefaultChatTitle => "Chat",
        I18nKey::ChatSessionDeleted => "Chat session deleted",

        // PDF Search
        I18nKey::SearchInPdf => "Search PDF",
        I18nKey::SearchInputPlaceholder => "Enter search terms...",

        // Native macOS Menu
        I18nKey::Hide => "Hide Lumen",
        I18nKey::HideOthers => "Hide Others",
        I18nKey::ShowAll => "Show All",
        I18nKey::Services => "Services",

        // File Library Sync Dialog
        I18nKey::FileLibraryInitRequiredTitle => "Initialize Remote File Storage",
        I18nKey::FileLibraryInitRequiredDesc => {
            "The remote file storage is empty and not initialized. Do you want to initialize it as the attachment storage for the current local library?"
        }
        I18nKey::FileLibraryInitializing => "Initializing remote file storage...",
        I18nKey::FileLibraryInitSuccess => "Remote file storage initialized successfully",
        I18nKey::FileLibraryInitFailed => "Failed to initialize remote file storage",
        I18nKey::FileLibraryUnidentified => {
            "Remote storage contains non-empty unrecognized files without an identity document; cannot synchronize safely"
        }
        I18nKey::FileLibraryIdentityMismatch => {
            "Remote file storage identity does not match current local database library; synchronization refused"
        }

        // Attachment Instant Restore & Conflict Notifications
        I18nKey::AttachmentRestoring => "Restoring attachment file from remote...",
        I18nKey::AttachmentPendingDownloadNotice => {
            "Attachment is pending on-demand download; attempting instant restore"
        }
        I18nKey::AttachmentUnrecoverableMissingNotice => {
            "Remote object does not exist; file cannot be recovered"
        }
        I18nKey::AttachmentFileConflictNotice => {
            "Both local and remote files have modified; automatic overwrite prevented"
        }
        I18nKey::AttachmentUnknownDivergenceNotice => {
            "Missing trusted baseline or diverging contents; automatic overwrite prevented"
        }
        I18nKey::AttachmentRestoreFailedNotice => {
            "Failed to restore attachment; check connection and permissions"
        }
        I18nKey::AttachmentOpenFailedNotice => {
            "Failed to open attachment with the external program"
        }
    }
}
