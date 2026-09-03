use crate::I18nKey;

pub fn translate(key: I18nKey) -> &'static str {
    match key {
        I18nKey::Library => "라이브러리",
        I18nKey::Subscription => "구독",

        // Native macOS Menu
        I18nKey::Hide => "Lumen 숨기기",
        I18nKey::HideOthers => "다른 것 숨기기",
        I18nKey::ShowAll => "모두 보기",
        I18nKey::Services => "서비스",
        I18nKey::AllLiterature => "모든 문헌",
        I18nKey::Uncategorized => "미분류",
        I18nKey::Trash => "휴지통",
        I18nKey::Tags => "태그",
        I18nKey::AllSubscription => "모든 구독",
        I18nKey::Unread => "읽지 않음",
        I18nKey::StatusReading => "읽는 중",
        I18nKey::StatusRead => "읽음",
        I18nKey::FolderNamePlaceholder => "폴더 이름",
        I18nKey::TagNamePlaceholder => "태그 이름",
        I18nKey::Version => "버전",
        I18nKey::EmptyFolder => "폴더가 비어 있습니다",
        I18nKey::NoMatchFound => "일치하는 문헌을 찾을 수 없습니다",
        I18nKey::SearchBoxPlaceholder => "문헌, 저자 또는 학술지 검색...",
        I18nKey::ManualAdd => "수동 추가",
        I18nKey::BibTeXImport => "BibTeX 가져오기",
        I18nKey::DoiImport => "DOI 가져오기",
        I18nKey::ArXivImport => "ArXiv 가져오기",
        I18nKey::DblpSearch => "DBLP 검색",
        I18nKey::DuplicateGroups => "Duplicate Groups",
        I18nKey::SyncConflicts => "Version Conflicts",
        I18nKey::NoDuplicatesFound => "No duplicates found",
        I18nKey::DuplicateSearch => "중복 문헌 검색",
        I18nKey::NewFolder => "새 폴더",
        I18nKey::EmptyTrash => "휴지통 비우기",
        I18nKey::Rename => "이름 바꾸기",
        I18nKey::Delete => "삭제",
        I18nKey::NewSubscription => "새 구독",
        I18nKey::OpenInBrowser => "브라우저에서 열기",
        I18nKey::MarkAsRead => "읽음으로 표시",
        I18nKey::MarkAsUnread => "읽지 않음으로 표시",
        I18nKey::UpdateSubscription => "업데이트",
        I18nKey::EditSubscription => "구독 편집",
        I18nKey::Unsubscribe => "구독 취소",
        I18nKey::AddSubscription => "구독 추가",
        I18nKey::UpdateAllSubscriptions => "모든 구독 업데이트",
        I18nKey::SubscriptionUpdated => "구독 {} 업데이트됨",
        I18nKey::SubscriptionUpdateFailed => "구독 {} 업데이트 실패: {}",
        I18nKey::NewSubFolder => "하위 폴더 만들기",
        I18nKey::Edit => "편집",
        I18nKey::Quit => "종료",
        I18nKey::PermanentDelete => "영구 삭제",
        I18nKey::CopyCitation => "인용 복사",
        I18nKey::FetchFrom => "가져오기 원본...",
        I18nKey::BatchFetchMetadata => "메타데이터 일괄 업데이트",
        I18nKey::AddTo => "추가 위치...",
        I18nKey::RestoreTo => "복원 위치...",
        I18nKey::RemoveFromFolder => "폴더에서 제거",
        I18nKey::RevealInFinder => "Finder에서 보기",
        I18nKey::RevealInExplorer => "파일 탐색기에서 보기",
        I18nKey::OpenPath => "경로 열기",
        I18nKey::ReplaceFile => "파일 교체",
        I18nKey::DeleteFile => "파일 삭제",
        I18nKey::Export => "내보내기",
        I18nKey::ExportAnnotatedPdf => "주석이 포함된 PDF 내보내기",
        I18nKey::ExportAnnotatedPdfSuccess => "PDF 내보내기 완료 (주석 포함)",
        I18nKey::ExportAnnotatedPdfFailed => "주석이 포함된 PDF 내보내기 실패",
        I18nKey::ExportAnnotatedPdfNoAnnotations => "내보낼 주석이 없는 문서입니다",
        I18nKey::SelectNewFile => "새 파일 선택",
        I18nKey::Confirm => "확인",
        I18nKey::LoadingMetadata => "데이터 가져오는 중...",
        I18nKey::FetchFailed => "가져오기 실패",
        I18nKey::Retry => "재시도",
        I18nKey::Close => "닫기",
        I18nKey::ConfirmFetch => "가져오기",
        I18nKey::FetchFromSource => "{}에서 가져오기",
        I18nKey::FetchPlaceholderDoi => "DOI를 입력하세요",
        I18nKey::FetchPlaceholderArxiv => "ArXiv ID를 입력하세요",
        I18nKey::FetchPlaceholderBibtex => "BibTeX를 붙여넣으세요",
        I18nKey::FetchPlaceholderDblp => "DBLP 검색",
        I18nKey::FetchPlaceholderOpenAlex => "OpenAlex 검색",
        I18nKey::NoContentOrInvalidFormat => "내용이 없거나 형식이 잘못되었습니다",
        I18nKey::ImportFailed => "가져오기 실패",

        I18nKey::LiteratureEditor => "문헌 편집기",
        I18nKey::AuthorPlaceholder => "저자 (쉼표로 구분)",
        I18nKey::JournalPlaceholder => "학술지/컨퍼런스/도서명",
        I18nKey::Month => "월",
        I18nKey::Day => "일",
        I18nKey::Publisher => "출판사",
        I18nKey::Field => "필드",
        I18nKey::LocalData => "로컬 데이터",
        I18nKey::RemoteData => "원격 데이터",
        I18nKey::SubscriptionEditor => "구독 편집기",
        I18nKey::FeedName => "이름",
        I18nKey::FeedUrl => "URL",
        I18nKey::UpdateInterval => "업데이트 간격",
        I18nKey::SubscriptionNamePlaceholder => "이름",
        I18nKey::SubscriptionUrlPlaceholder => "RSS URL",
        I18nKey::UpdateIntervalPlaceholder => "업데이트 간격 (시간)",
        I18nKey::Add => "추가",

        I18nKey::SelectedSubscriptionCount => "{}개의 구독 항목 선택됨",
        I18nKey::AddToLibrary => "+ 라이브러리에 추가",
        I18nKey::NoSubscriptionSelected => "선택된 구독이 없습니다",
        I18nKey::NoAbstract => "초록 없음",

        I18nKey::CopyCitationTitle => "인용 복사",
        I18nKey::Style => "스타일",
        I18nKey::Preview => "미리보기",
        I18nKey::NoLiteratureSelectedForCitation => "선택된 문헌이 없습니다",
        I18nKey::CitationError => "인용 생성 실패",
        I18nKey::CitationBibTeX => "BibTeX",
        I18nKey::CitationIeee => "IEEE",
        I18nKey::CopiedToClipboard => "클립보드에 복사되었습니다",
        I18nKey::CitationSettings => "인용 설정",
        I18nKey::AbbreviateJournalInCitation => "인용에서 저널명 약어 사용",

        I18nKey::Type => "유형",
        I18nKey::TypeArticle => "학술지 논문",
        I18nKey::TypeBook => "도서",
        I18nKey::TypeConference => "컨퍼런스 논문",
        I18nKey::TypeThesis => "학위 논문",
        I18nKey::TypePreprint => "사전 출판",
        I18nKey::TypeTechnicalReport => "기술 보고서",
        I18nKey::TypeWebpage => "웹페이지",
        I18nKey::TypeOther => "기타",
        I18nKey::Title => "제목",
        I18nKey::Authors => "저자",
        I18nKey::Journal => "학술지",
        I18nKey::Year => "연도",
        I18nKey::Volume => "Vol.",
        I18nKey::Issue => "No.",
        I18nKey::Pages => "Pages",
        I18nKey::Url => "URL",
        I18nKey::Doi => "DOI",
        I18nKey::ArXiv => "ArXiv",
        I18nKey::Abstract => "초록",
        I18nKey::Notes => "노트",
        I18nKey::Attachments => "첨부 파일",
        I18nKey::Folders => "폴더",
        I18nKey::NoLiteratureSelected => "선택된 문헌이 없습니다",
        I18nKey::SelectedCount => "{}개의 문헌이 선택됨",
        I18nKey::MainFile => "주요 파일",
        I18nKey::Attachment => "첨부 파일",
        I18nKey::SetAsMainFile => "주요 파일로 설정",
        I18nKey::SetAsAttachment => "첨부 파일로 설정",
        I18nKey::Expand => "모두 펼치기 ↓",
        I18nKey::Collapse => "접기 ↑",
        I18nKey::Publication => "출판",
        I18nKey::PublicationAbbreviation => "저널 약어",
        I18nKey::RelatedLiterature => "관련 문헌",
        I18nKey::AddCitation => "인용 추가",
        I18nKey::References => "참고문헌",
        I18nKey::CitedBy => "피인용",
        I18nKey::Settings => "설정",
        I18nKey::Language => "언어",
        I18nKey::Appearance => "모양",
        I18nKey::UiScale => "UI 크기",
        I18nKey::LogLevel => "로그 레벨",
        I18nKey::NotificationLevel => "알림 레벨",
        I18nKey::ThemeStyle => "테마 스타일",
        I18nKey::Theme => "테마",
        I18nKey::Dark => "어둡게",
        I18nKey::Light => "밝게",
        I18nKey::System => "시스템 설정",
        I18nKey::General => "일반",
        I18nKey::Sync => "클라우드 동기화",
        I18nKey::About => "정보",
        I18nKey::Cancel => "취소",
        I18nKey::Save => "저장",
        I18nKey::LibrarySettings => "라이브러리 설정",
        I18nKey::AttachmentDir => "첨부 파일 저장 경로",
        I18nKey::AttachmentDirDesc => "모든 PDF 및 첨부 파일이 이 디렉토리에 저장됩니다",
        I18nKey::DatabaseDir => "데이터베이스 디렉토리",
        I18nKey::DatabaseDirDesc => "데이터베이스 파일이 저장되는 위치",
        I18nKey::FilenameTemplate => "파일명 형식",
        I18nKey::FilenameTemplateDesc => {
            "Custom renaming rules for attachments. Available variables: {title}, {author}, {year}, {publication}, {firstname}, {lastname}, {firstchartitle}. Supports using '/' for folder hierarchy."
        }
        I18nKey::GeneralOptions => "일반 옵션",
        I18nKey::CloudSyncDesc => {
            "클라우드 동기화 기능은 개발 중입니다. 향후 WebDAV, S3 및 다중 기기 동기화를 지원할 예정입니다."
        }
        I18nKey::AboutDesc => {
            "GPUI로 구축된 고성능 문헌 관리 앱입니다. 깔끔하고 부드러우며 강력한 학술 독서 및 연구 경험에 집중합니다."
        }
        I18nKey::BatchRename => "Batch Rename",
        I18nKey::BatchRenameCompleted => "일괄 이름 변경 완료: 성공 {}개, 건너뜀 {}개",
        I18nKey::BatchRenameCompletedWithFailures => {
            "일괄 이름 변경 완료: 성공 {}개, 건너뜀 {}개, 실패 {}개. {}"
        }
        I18nKey::BatchRenameFailed => "일괄 이름 변경을 계속할 수 없습니다: {}",
        I18nKey::BatchRenameSourceNotRegularFile => "원본 파일이 없거나 일반 파일이 아닙니다: {}",
        I18nKey::BatchRenameSourceNameUnreadable => "원본 파일 이름을 읽을 수 없습니다",
        I18nKey::BatchRenameTargetExists => "대상 파일이 이미 존재합니다: {}",
        I18nKey::BatchRenameRenameFailed => "파일 이름 변경 실패: {}",
        I18nKey::BatchRenameSaveFailed => {
            "파일 이름은 변경되었지만 기록을 저장할 수 없어 계속할 수 없습니다: {}"
        }
        I18nKey::CleanupOrphanedFiles => "Cleanup Orphaned Files",
        I18nKey::Copyright => "© 2026 Lumen. 모든 권리 보유.",
        // Sort
        I18nKey::SortBy => "정렬",
        I18nKey::SortByTitle => "제목",
        I18nKey::SortByAuthor => "저자",
        I18nKey::SortByYear => "연도",
        I18nKey::SortByJournal => "저널",
        I18nKey::SortAscending => "오름차순",
        I18nKey::SortDescending => "내림차순",

        I18nKey::UpdatedAt => "업데이트 날짜",

        // Sync
        I18nKey::SyncMetadata => "메타데이터 동기화",
        I18nKey::SyncAttachments => "첨부 파일 동기화",
        I18nKey::FileSyncLastRun => "마지막 파일 동기화",
        I18nKey::FileSyncSummaryUnavailable => "파일 동기화 요약을 사용할 수 없음",
        I18nKey::SyncSkippedBusy => "동기화가 진행 중이어서 이번 요청이 시작되지 않았습니다",
        I18nKey::SyncUploaded => "업로드",
        I18nKey::SyncDownloaded => "다운로드",
        I18nKey::SyncDeleted => "삭제",
        I18nKey::SyncFailures => "실패",
        I18nKey::SyncWaiting => "대기",
        I18nKey::SyncPendingDownload => "다운로드 대기",
        I18nKey::SyncUnknownDivergence => "불일치",
        I18nKey::SyncUnrecoverable => "복구 불가",
        I18nKey::DatabaseSyncInProgress => {
            "데이터베이스 동기화를 업그레이드하는 중입니다. 파일 동기화는 계속 사용할 수 있습니다"
        }
        I18nKey::DatabaseSyncNeedsInitialization => "데이터베이스 동기화 초기화가 필요합니다",
        I18nKey::DatabaseSyncNeedsAdoption => "원격 데이터베이스 채택이 필요합니다",
        I18nKey::DatabaseSyncIdentityMismatch => "데이터베이스 ID가 일치하지 않습니다",
        I18nKey::DatabaseSyncConflict => "데이터베이스 동기화 충돌",
        I18nKey::DatabaseSyncPartialFailure => "데이터베이스 동기화 일부 실패",
        I18nKey::DatabaseSyncError => "데이터베이스 동기화 오류",
        I18nKey::DatabaseSyncUploaded => "업로드됨",
        I18nKey::DatabaseSyncDownloaded => "다운로드됨",
        I18nKey::DatabaseSyncConflictsCount => "충돌",
        I18nKey::DatabaseSyncFailuresCount => "실패",
        I18nKey::TestConnection => "연결 테스트",
        I18nKey::WebDavSettings => "WebDAV 설정",
        I18nKey::EnableWebDav => "WebDAV 사용",
        I18nKey::DatabaseSettings => "데이터베이스 설정",
        I18nKey::EndpointUrl => "서버 주소",
        I18nKey::Username => "사용자 이름",
        I18nKey::Password => "비밀번호",
        I18nKey::RemotePath => "원격 경로",
        I18nKey::Host => "호스트",
        I18nKey::Port => "포트",
        I18nKey::DatabaseName => "데이터베이스 이름",
        I18nKey::EnableSSL => "SSL 사용",
        I18nKey::UseRemoteDatabase => "원격 데이터베이스 사용",
        I18nKey::ConnectionSuccess => "연결 성공",
        I18nKey::ConnectionFailed => "연결 실패",
        I18nKey::SearchOrCreateTags => "태그 검색 또는 생성...",
        I18nKey::CreateTag => "\"{}\" 생성",

        // PDF Viewer
        I18nKey::PdfViewerSettings => "PDF 뷰어 설정",
        I18nKey::PdfViewerSettingsDesc => "PDF 파일을 여는 응용 프로그램 사용자 정의",
        I18nKey::UseCustomPdfViewer => "사용자 정의 PDF 뷰어 사용",
        I18nKey::PdfViewerPathMacos => "macOS 응용 프로그램",
        I18nKey::PdfViewerPathWindows => "Windows 프로그램",
        I18nKey::SelectMetadataCandidate => "가장 일치하는 메타데이터 선택",

        // Network Proxy
        I18nKey::NetworkProxySettings => "네트워크 프록시",
        I18nKey::EnableProxyServer => "사용자 정의 프록시 서버 사용",
        I18nKey::ProxyAddress => "프록시 서버 주소",
        I18nKey::ProxyDesc => "HTTP, HTTPS 또는 SOCKS5 프로토콜 지원 (예: http://127.0.0.1:7890)",

        // Service Errors
        // Error/Notification
        I18nKey::FileNotFoundTitle => "파일을 찾을 수 없음",
        I18nKey::FileNotFoundMsg => "경로 {:?}가 존재하지 않습니다",
        I18nKey::DataConsistentTitle => "데이터 일치",
        I18nKey::DataConsistentMsg => {
            "가져온 메타데이터가 로컬 데이터와 완전히 일치합니다. 병합이 필요하지 않습니다."
        }
        I18nKey::LiteratureMergedTitle => "문헌 병합됨",
        I18nKey::LiteratureMergedMsg => {
            "\"{}\"의 복사본이 원본 문헌과 동일하여 휴지통으로 이동되었습니다."
        }

        // Fetch Error Tips
        I18nKey::FetchFailedArxiv => {
            "이 문헌에 ArXiv ID 또는 관련 링크가 없습니다. ArXiv에서 메타데이터를 가져올 수 없습니다."
        }
        I18nKey::FetchFailedDblp => "문헌 제목이 비어 있습니다. DBLP에서 검색할 수 없습니다.",
        I18nKey::FetchFailedCrossref => {
            "이 문헌에 DOI 필드가 없거나 비어 있습니다. Crossref에서 메타데이터를 가져올 수 없습니다."
        }
        I18nKey::FetchFailedOpenAlex => {
            "DOI와 제목이 모두 비어 있습니다. OpenAlex에서 검색할 수 없습니다."
        }

        // Batch Update
        I18nKey::BatchUpdatingMetadata => "메타데이터 일괄 업데이트 중 ({}/{})",

        // Settings - Translation
        I18nKey::TranslationSettings => "번역 설정",
        I18nKey::TranslationSettingsDesc => "PDF 리더의 번역 엔진 및 API 키를 구성합니다.",
        I18nKey::TranslationEngine => "번역 엔진",
        I18nKey::TranslationSettingsTab => "번역",
        I18nKey::NoApiKeyRequired => "이 엔진은 API 키가 필요 없으며 바로 사용할 수 있습니다.",
        I18nKey::AiBackend => "AI 백엔드",
        I18nKey::NiuTransApiKey => "NiuTrans API Key",
        I18nKey::GoogleApiKey => "Google Cloud API Key",
        I18nKey::BaiduApiKey => "Baidu AppID#Key",
        I18nKey::YoudaoApiKey => "Youdao AppID#Key",
        I18nKey::DeepLApiKey => "DeepL API Key",
        I18nKey::AiApiKey => "AI API Key",
        I18nKey::AiApiBase => "API Base URL",
        I18nKey::AiModel => "모델",
        I18nKey::AiContextWindow => "컨텍스트 윈도우",
        I18nKey::AiCompressionStrategy => "압축 전략",
        I18nKey::SlidingWindow => "슬라이딩 윈도우",
        I18nKey::SummaryCompression => "요약 압축",
        I18nKey::AiBackendName => "이름",
        I18nKey::AiBackendType => "유형",
        I18nKey::AiAddBackend => "백엔드 추가",
        I18nKey::AiActive => "활성",
        I18nKey::AiNoBackends => "AI 백엔드가 구성되지 않음",
        I18nKey::TargetLanguage => "번역 대상 언어",
        I18nKey::EngineGoogleFree => "Google (무료)",
        I18nKey::EngineBingFree => "Bing (무료)",
        I18nKey::EngineGoogleCloud => "Google Cloud",
        I18nKey::EngineNiuTrans => "NiuTrans",
        I18nKey::EngineBaidu => "Baidu",
        I18nKey::EngineYoudao => "Youdao",
        I18nKey::EngineDeeplFree => "DeepL Free",
        I18nKey::EngineDeeplPro => "DeepL Pro",
        I18nKey::EngineAi => "AI",
        I18nKey::AiBackendsSettingsTab => "AI 백엔드",
        I18nKey::AiChatSettingsTab => "AI 채팅",
        I18nKey::DefaultSystemPrompt => "기본 시스템 프롬프트",
        I18nKey::BackendSelection => "백엔드 선택",
        I18nKey::ActiveBackend => "현재 백엔드",
        I18nKey::EnableThinking => "사고 과정 활성화",
        I18nKey::InternalReaderDesc => "외부 리더를 비활성화하면 PDF가 내장 리더로 열립니다",
        // PDF View - Notes
        I18nKey::EditNotesMarkdown => "메모 편집 (Markdown)",

        // Feed
        // Bookmark
        I18nKey::UnnamedBookmark => "이름 없는 북마크",
        I18nKey::SelectMacosPdfReader => "macOS PDF 리더 선택",
        I18nKey::SelectWindowsPdfReader => "Windows PDF 리더 선택",

        // Pdf Viewer
        I18nKey::NotePlaceholder => "메모 입력...",
        I18nKey::ViewNote => "메모 보기",
        I18nKey::AddNote => "메모 추가",
        I18nKey::Highlight => "하이라이트",
        I18nKey::Underline => "밑줄",
        I18nKey::LoadingOutline => "목차 로딩 중...",
        I18nKey::NoOutline => "이 문서에는 목차가 없습니다",
        I18nKey::RectangleAnnotation => "사각형",
        I18nKey::PageRange => "{}-{} 페이지",
        I18nKey::SinglePage => "{} 페이지",
        I18nKey::SelectTextToTranslate => "텍스트를 선택하여 번역",
        I18nKey::Translate => "번역",
        I18nKey::OriginalSection => "원문",
        I18nKey::TranslationSection => "번역",
        I18nKey::Translating => "번역 중...",
        I18nKey::TranslationPending => "번역 대기 중",
        I18nKey::NoNotes => "메모 없음",
        I18nKey::CopyAsImage => "이미지로 복사",
        I18nKey::PdfEngineError => "PDF 렌더링 엔진 오류",
        I18nKey::CloseWindow => "창 닫기",
        I18nKey::TranslationNotImplemented => "번역 기능이 구현되지 않았습니다",
        I18nKey::CreatePip => "새 PIP",
        I18nKey::DeletePage => "페이지 삭제",
        I18nKey::SaveAsImage => "이미지로 저장",

        // Pdf Viewer - AI Chat
        I18nKey::Chat => "AI 채팅",
        I18nKey::NewChat => "새 채팅",
        I18nKey::ChatInputPlaceholder => "메시지를 입력하세요...",
        I18nKey::NoChatSessions => "채팅이 없습니다",
        I18nKey::DeleteChat => "채팅 삭제",
        I18nKey::SendSelection => "선택 텍스트 전송",
        I18nKey::QuoteLabel => "인용",
        I18nKey::AttachFile => "파일 첨부",
        I18nKey::NoAttachments => "첨부 파일 없음",
        I18nKey::AiThinking => "생각 중...",
        I18nKey::BackToSessions => "세션 목록으로",
        I18nKey::EditSystemPrompt => "시스템 프롬프트 편집",
        I18nKey::DefaultChatTitle => "채팅",
        I18nKey::ChatSessionDeleted => "채팅이 삭제되었습니다",

        // PDF Search
        I18nKey::SearchInPdf => "PDF 검색",
        I18nKey::SearchInputPlaceholder => "검색어 입력...",
        I18nKey::SyncMetadataTab => "메타데이터 동기화",
        I18nKey::SyncAttachmentTab => "첨부파일 동기화",
        I18nKey::EnableGoogleDrive => "Google Drive 사용",
        I18nKey::GoogleDriveDesc => "첨부파일을 Google Drive에 동기화",
        I18nKey::ClientId => "클라이언트 ID",
        I18nKey::ClientSecret => "클라이언트 시크릿",
        I18nKey::Authorize => "인증",
        I18nKey::DataManagement => "Data Management",
        I18nKey::ClearLocalDb => "Clear Local Database",
        I18nKey::ClearLocalFiles => "Clear Local Files",
        I18nKey::CheckLocalFiles => "Check Local Files",
        I18nKey::ClearCloudDb => "Clear Cloud Database",
        I18nKey::ClearCloudFiles => "Clear Cloud Files",
        I18nKey::PurgeSyncedDeletions => "삭제된 데이터 정리",
        I18nKey::PurgeDeletedData => "삭제된 데이터 완전 정리 (로컬 + 원격)",

        // File Library Sync Dialog
        I18nKey::FileLibraryInitRequiredTitle => "원격 파일 저장소 초기화",
        I18nKey::FileLibraryInitRequiredDesc => {
            "원격 저장소가 비어있고 초기화되지 않았습니다. 현재 로컬 라이브러리의 첨부파일 저장소로 초기화하시겠습니까?"
        }
        I18nKey::FileLibraryInitializing => "원격 파일 저장소 초기화 중...",
        I18nKey::FileLibraryInitSuccess => "원격 파일 저장소 초기화 완료",
        I18nKey::FileLibraryInitFailed => "원격 파일 저장소 초기화 실패",
        I18nKey::FileLibraryUnidentified => {
            "원격 저장소에 식별되지 않은 파일이 존재합니다. 안전을 위해 동기화할 수 없습니다"
        }
        I18nKey::FileLibraryIdentityMismatch => {
            "원격 파일 저장소의 라이브러리 ID가 로컬 데이터베이스와 일치하지 않아 동기화가 거부되었습니다"
        }

        // Attachment Instant Restore & Conflict Notifications
        I18nKey::AttachmentRestoring => "원격에서 첨부파일 복원 중...",
        I18nKey::AttachmentPendingDownloadNotice => {
            "주문형 다운로드 대기 중입니다. 즉시 복원을 시도합니다"
        }
        I18nKey::AttachmentUnrecoverableMissingNotice => {
            "원격 객체가 존재하지 않아 파일을 복원할 수 없습니다"
        }
        I18nKey::AttachmentFileConflictNotice => {
            "로컬 및 원격 파일이 모두 수정되어 자동 덮어쓰기가 차단되었습니다"
        }
        I18nKey::AttachmentUnknownDivergenceNotice => {
            "신뢰할 수 있는 기준선이 없거나 내용이 분기되어 자동 덮어쓰기가 차단되었습니다"
        }
        I18nKey::AttachmentRestoreFailedNotice => {
            "첨부파일 복원에 실패했습니다. 연결 및 권한을 확인하세요"
        }
        I18nKey::AttachmentOpenFailedNotice => "외부 프로그램으로 첨부 파일을 열지 못했습니다",
    }
}
