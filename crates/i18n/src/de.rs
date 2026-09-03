use crate::I18nKey;

pub fn translate(key: I18nKey) -> &'static str {
    match key {
        I18nKey::Library => "Bibliothek",
        I18nKey::Subscription => "Abonnement",

        // Native macOS Menu
        I18nKey::Hide => "Lumen ausblenden",
        I18nKey::HideOthers => "Andere ausblenden",
        I18nKey::ShowAll => "Alle einblenden",
        I18nKey::Services => "Dienste",
        I18nKey::AllLiterature => "Alle Literatur",
        I18nKey::Uncategorized => "Unkategorisiert",
        I18nKey::Trash => "Papierkorb",
        I18nKey::Tags => "Tags",
        I18nKey::AllSubscription => "Alle Abonnements",
        I18nKey::Unread => "Ungelesen",
        I18nKey::StatusReading => "Lesend",
        I18nKey::StatusRead => "Gelesen",
        I18nKey::FolderNamePlaceholder => "Ordnername",
        I18nKey::TagNamePlaceholder => "Tag-Name",
        I18nKey::Version => "Version",
        I18nKey::EmptyFolder => "Ordner ist leer",
        I18nKey::NoMatchFound => "Keine Treffer gefunden",
        I18nKey::SearchBoxPlaceholder => "Literatur, Autoren oder Zeitschriften suchen...",
        I18nKey::ManualAdd => "Manuell hinzufügen",
        I18nKey::BibTeXImport => "BibTeX Import",
        I18nKey::DoiImport => "DOI Import",
        I18nKey::ArXivImport => "ArXiv Import",
        I18nKey::DblpSearch => "DBLP Suche",
        I18nKey::DuplicateGroups => "Duplicate Groups",
        I18nKey::SyncConflicts => "Version Conflicts",
        I18nKey::NoDuplicatesFound => "No duplicates found",
        I18nKey::DuplicateSearch => "Duplikate suchen",
        I18nKey::NewFolder => "Neuer Ordner",
        I18nKey::EmptyTrash => "Papierkorb leeren",
        I18nKey::Rename => "Umbenennen",
        I18nKey::Delete => "Löschen",
        I18nKey::NewSubscription => "Neues Abonnement",
        I18nKey::OpenInBrowser => "Im Browser öffnen",
        I18nKey::MarkAsRead => "Als gelesen markieren",
        I18nKey::MarkAsUnread => "Als ungelesen markieren",
        I18nKey::UpdateSubscription => "Aktualisieren",
        I18nKey::EditSubscription => "Abonnement bearbeiten",
        I18nKey::Unsubscribe => "Abbestellen",
        I18nKey::AddSubscription => "Abonnement hinzufügen",
        I18nKey::UpdateAllSubscriptions => "Alle Abonnements aktualisieren",
        I18nKey::SubscriptionUpdated => "Abonnement {} aktualisiert",
        I18nKey::SubscriptionUpdateFailed => "Aktualisierung von {} fehlgeschlagen: {}",
        I18nKey::NewSubFolder => "Unterordner erstellen",
        I18nKey::Edit => "Bearbeiten",
        I18nKey::Quit => "Beenden",
        I18nKey::PermanentDelete => "Endgültig löschen",
        I18nKey::CopyCitation => "Zitat kopieren",
        I18nKey::FetchFrom => "Abrufen von...",
        I18nKey::BatchFetchMetadata => "Metadaten stapelweise aktualisieren",
        I18nKey::AddTo => "Hinzufügen zu...",
        I18nKey::RestoreTo => "Wiederherstellen in...",
        I18nKey::RemoveFromFolder => "Aus Ordner entfernen",
        I18nKey::RevealInFinder => "Im Finder anzeigen",
        I18nKey::RevealInExplorer => "Im Explorer anzeigen",
        I18nKey::OpenPath => "Pfad öffnen",
        I18nKey::ReplaceFile => "Datei ersetzen",
        I18nKey::DeleteFile => "Datei löschen",
        I18nKey::Export => "Exportieren",
        I18nKey::ExportAnnotatedPdf => "PDF mit Anmerkungen exportieren",
        I18nKey::ExportAnnotatedPdfSuccess => "PDF exportiert (mit Anmerkungen)",
        I18nKey::ExportAnnotatedPdfFailed => "Export des PDF mit Anmerkungen fehlgeschlagen",
        I18nKey::ExportAnnotatedPdfNoAnnotations => {
            "Dieses Dokument enthält keine Anmerkungen zum Exportieren"
        }
        I18nKey::SelectNewFile => "Neue Datei wählen",
        I18nKey::Confirm => "OK",
        I18nKey::LoadingMetadata => "Metadaten werden abgerufen...",
        I18nKey::FetchFailed => "Abrufen fehlgeschlagen",
        I18nKey::Retry => "Wiederholen",
        I18nKey::Close => "Schließen",
        I18nKey::ConfirmFetch => "Jetzt abrufen",
        I18nKey::FetchFromSource => "Abrufen von {}",
        I18nKey::FetchPlaceholderDoi => "DOI eingeben",
        I18nKey::FetchPlaceholderArxiv => "ArXiv-ID eingeben",
        I18nKey::FetchPlaceholderBibtex => "BibTeX einfügen",
        I18nKey::FetchPlaceholderDblp => "In DBLP suchen",
        I18nKey::FetchPlaceholderOpenAlex => "In OpenAlex suchen",
        I18nKey::NoContentOrInvalidFormat => "Inhalt leer oder ungültiges Format",
        I18nKey::ImportFailed => "Import fehlgeschlagen",

        I18nKey::LiteratureEditor => "Literatur-Editor",
        I18nKey::AuthorPlaceholder => "Autoren (kommagetrennt)",
        I18nKey::JournalPlaceholder => "Zeitschrift / Konferenz / Buch",
        I18nKey::Month => "Monat",
        I18nKey::Day => "Tag",
        I18nKey::Publisher => "Verlag",
        I18nKey::Field => "Feld",
        I18nKey::LocalData => "Lokale Daten",
        I18nKey::RemoteData => "Remote-Daten",
        I18nKey::SubscriptionEditor => "Abonnement-Editor",
        I18nKey::FeedName => "Name",
        I18nKey::FeedUrl => "URL",
        I18nKey::UpdateInterval => "Aktualisierungsintervall",
        I18nKey::SubscriptionNamePlaceholder => "Name",
        I18nKey::SubscriptionUrlPlaceholder => "RSS-URL",
        I18nKey::UpdateIntervalPlaceholder => "Intervall (Stunden)",
        I18nKey::Add => "Hinzufügen",

        I18nKey::SelectedSubscriptionCount => "{} Abonnements ausgewählt",
        I18nKey::AddToLibrary => "+ In Bibliothek übertragen",
        I18nKey::NoSubscriptionSelected => "Kein Abonnement ausgewählt",
        I18nKey::NoAbstract => "Keine Zusammenfassung verfügbar",

        I18nKey::CopyCitationTitle => "Zitat kopieren",
        I18nKey::Style => "Stil",
        I18nKey::Preview => "Vorschau",
        I18nKey::NoLiteratureSelectedForCitation => "Keine Literatur ausgewählt",
        I18nKey::CitationError => "Zitatgenerierung fehlgeschlagen",
        I18nKey::CitationBibTeX => "BibTeX",
        I18nKey::CitationIeee => "IEEE",
        I18nKey::CopiedToClipboard => "In die Zwischenablage kopiert",
        I18nKey::CitationSettings => "Zitier-Einstellungen",
        I18nKey::AbbreviateJournalInCitation => "Journalnamen in Zitaten abkürzen",

        I18nKey::Type => "Typ",
        I18nKey::TypeArticle => "Zeitschriftenartikel",
        I18nKey::TypeBook => "Buch",
        I18nKey::TypeConference => "Konferenzbeitrag",
        I18nKey::TypeThesis => "Abschlussarbeit",
        I18nKey::TypePreprint => "Preprint",
        I18nKey::TypeTechnicalReport => "Technischer Bericht",
        I18nKey::TypeWebpage => "Webseite",
        I18nKey::TypeOther => "Andere",
        I18nKey::Title => "Titel",
        I18nKey::Authors => "Autoren",
        I18nKey::Journal => "Zeitschrift",
        I18nKey::Year => "Jahr",
        I18nKey::Volume => "Vol.",
        I18nKey::Issue => "No.",
        I18nKey::Pages => "Pages",
        I18nKey::Url => "URL",
        I18nKey::Doi => "DOI",
        I18nKey::ArXiv => "ArXiv",
        I18nKey::Abstract => "Zusammenfassung",
        I18nKey::Notes => "Notizen",
        I18nKey::Attachments => "Anhänge",
        I18nKey::Folders => "Ordner",
        I18nKey::NoLiteratureSelected => "Keine Literatur ausgewählt",
        I18nKey::SelectedCount => "{} Einträge ausgewählt",
        I18nKey::MainFile => "Hauptdatei",
        I18nKey::Attachment => "Anhang",
        I18nKey::SetAsMainFile => "Als Hauptdatei festlegen",
        I18nKey::SetAsAttachment => "Als Anhang festlegen",
        I18nKey::Expand => "Alle ausklappen ↓",
        I18nKey::Collapse => "Einklappen ↑",
        I18nKey::Publication => "Veröffentlichung",
        I18nKey::PublicationAbbreviation => "Zeitschriften-Abkürzung",
        I18nKey::RelatedLiterature => "Verwandte Literatur",
        I18nKey::AddCitation => "Zitat hinzufügen",
        I18nKey::References => "Referenzen",
        I18nKey::CitedBy => "Zitiert von",
        I18nKey::Settings => "Einstellungen",
        I18nKey::Language => "Sprache",
        I18nKey::Appearance => "Aussehen",
        I18nKey::UiScale => "UI Skalierung",
        I18nKey::LogLevel => "Log Level",
        I18nKey::NotificationLevel => "Benachrichtigungsstufe",
        I18nKey::ThemeStyle => "Themenstil",
        I18nKey::Theme => "Thema",
        I18nKey::Dark => "Dunkel",
        I18nKey::Light => "Hell",
        I18nKey::System => "System",
        I18nKey::General => "Allgemein",
        I18nKey::Sync => "Synchronisation",
        I18nKey::About => "Über",
        I18nKey::Cancel => "Abbrechen",
        I18nKey::Save => "Speichern",
        I18nKey::LibrarySettings => "Bibliothek-Einstellungen",
        I18nKey::AttachmentDir => "Anhang-Verzeichnis",
        I18nKey::AttachmentDirDesc => "PDFs und Anhänge werden hier gespeichert",
        I18nKey::DatabaseDir => "Datenbank-Verzeichnis",
        I18nKey::DatabaseDirDesc => "Speicherort der Datenbankdateien",
        I18nKey::FilenameTemplate => "Filename Template",
        I18nKey::FilenameTemplateDesc => {
            "Custom renaming rules for attachments. Available variables: {title}, {author}, {year}, {publication}, {firstname}, {lastname}, {firstchartitle}. Supports using '/' for folder hierarchy."
        }
        I18nKey::GeneralOptions => "Allgemeine Optionen",
        I18nKey::CloudSyncDesc => "Cloud-Synchronisation wird entwickelt.",
        I18nKey::AboutDesc => "Hochleistungs-Literaturverwaltung auf GPUI-Basis.",
        I18nKey::BatchRename => "Batch Rename",
        I18nKey::BatchRenameCompleted => {
            "Stapelumbenennung abgeschlossen: {} erfolgreich, {} übersprungen"
        }
        I18nKey::BatchRenameCompletedWithFailures => {
            "Stapelumbenennung abgeschlossen: {} erfolgreich, {} übersprungen, {} fehlgeschlagen. {}"
        }
        I18nKey::BatchRenameFailed => "Stapelumbenennung konnte nicht fortgesetzt werden: {}",
        I18nKey::BatchRenameSourceNotRegularFile => {
            "Quelldatei fehlt oder ist keine reguläre Datei: {}"
        }
        I18nKey::BatchRenameSourceNameUnreadable => "Quelldateiname kann nicht gelesen werden",
        I18nKey::BatchRenameTargetExists => "Zieldatei ist bereits vorhanden: {}",
        I18nKey::BatchRenameRenameFailed => "Umbenennen der Datei fehlgeschlagen: {}",
        I18nKey::BatchRenameSaveFailed => {
            "Dateien wurden umbenannt, aber Datensätze nicht gespeichert; Fortsetzung nicht möglich: {}"
        }
        I18nKey::CleanupOrphanedFiles => "Cleanup Orphaned Files",
        I18nKey::Copyright => "© 2026 Lumen. Alle Rechte vorbehalten.",
        // Sort
        I18nKey::SortBy => "Sortieren",
        I18nKey::SortByTitle => "Titel",
        I18nKey::SortByAuthor => "Autor",
        I18nKey::SortByYear => "Jahr",
        I18nKey::SortByJournal => "Zeitschrift",
        I18nKey::SortAscending => "Aufsteigend",
        I18nKey::SortDescending => "Absteigend",

        I18nKey::UpdatedAt => "Aktualisiert am",

        // Sync
        I18nKey::SyncMetadata => "Metadaten synchronisieren",
        I18nKey::SyncAttachments => "Anhänge synchronisieren",
        I18nKey::FileSyncLastRun => "Letzte Dateisynchronisierung",
        I18nKey::FileSyncSummaryUnavailable => "Dateisync-Zusammenfassung nicht verfügbar",
        I18nKey::SyncSkippedBusy => "Synchronisierung läuft bereits, Anfrage nicht gestartet",
        I18nKey::SyncUploaded => "Hochgeladen",
        I18nKey::SyncDownloaded => "Heruntergeladen",
        I18nKey::SyncDeleted => "Gelöscht",
        I18nKey::SyncFailures => "Fehler",
        I18nKey::SyncWaiting => "Wartend",
        I18nKey::SyncPendingDownload => "Ausstehender Download",
        I18nKey::SyncUnknownDivergence => "Abweichungen",
        I18nKey::SyncUnrecoverable => "Nicht behebbar",
        I18nKey::DatabaseSyncInProgress => {
            "Datenbanksynchronisierung wird aktualisiert; Dateisynchronisierung bleibt verfügbar"
        }
        I18nKey::DatabaseSyncNeedsInitialization => {
            "Datenbanksynchronisierung muss initialisiert werden"
        }
        I18nKey::DatabaseSyncNeedsAdoption => "Datenbanksynchronisierung muss übernommen werden",
        I18nKey::DatabaseSyncIdentityMismatch => "Datenbankidentität stimmt nicht überein",
        I18nKey::DatabaseSyncConflict => "Konflikte bei der Datenbanksynchronisierung",
        I18nKey::DatabaseSyncPartialFailure => "Datenbanksynchronisierung teilweise fehlgeschlagen",
        I18nKey::DatabaseSyncError => "Fehler bei der Datenbanksynchronisierung",
        I18nKey::DatabaseSyncUploaded => "Hochgeladen",
        I18nKey::DatabaseSyncDownloaded => "Heruntergeladen",
        I18nKey::DatabaseSyncConflictsCount => "Konflikte",
        I18nKey::DatabaseSyncFailuresCount => "Fehler",
        I18nKey::TestConnection => "Verbindung testen",
        I18nKey::WebDavSettings => "WebDAV Einstellungen",
        I18nKey::EnableWebDav => "WebDAV aktivieren",
        I18nKey::DatabaseSettings => "Datenbankeinstellungen",
        I18nKey::EndpointUrl => "Server-Adresse",
        I18nKey::Username => "Benutzername",
        I18nKey::Password => "Passwort",
        I18nKey::RemotePath => "Remote-Pfad",
        I18nKey::Host => "Host",
        I18nKey::Port => "Port",
        I18nKey::DatabaseName => "Datenbankname",
        I18nKey::EnableSSL => "SSL aktivieren",
        I18nKey::UseRemoteDatabase => "Remote-Datenbank verwenden",
        I18nKey::ConnectionSuccess => "Verbindung erfolgreich",
        I18nKey::ConnectionFailed => "Verbindung fehlgeschlagen",
        I18nKey::SearchOrCreateTags => "Tags suchen oder erstellen...",
        I18nKey::CreateTag => "\"{}\" erstellen",

        // PDF Viewer
        I18nKey::PdfViewerSettings => "PDF-Viewer Einstellungen",
        I18nKey::PdfViewerSettingsDesc => "Anwendung zum Öffnen von PDF-Dateien anpassen",
        I18nKey::UseCustomPdfViewer => "Benutzerdefinierten PDF-Viewer verwenden",
        I18nKey::PdfViewerPathMacos => "macOS Anwendung",
        I18nKey::PdfViewerPathWindows => "Windows Programm",
        I18nKey::SelectMetadataCandidate => "Wählen Sie den besten Metadaten-Kandidaten aus",

        I18nKey::NetworkProxySettings => "Network Proxy",
        I18nKey::EnableProxyServer => "Enable Custom Proxy Server",
        I18nKey::ProxyAddress => "Proxy Server Address",
        I18nKey::ProxyDesc => "Supports HTTP, HTTPS or SOCKS5 protocol, e.g. http://127.0.0.1:7890",

        // Service Errors
        // Error/Notification
        I18nKey::FileNotFoundTitle => "Datei nicht gefunden",
        I18nKey::FileNotFoundMsg => "Pfad {:?} existiert nicht",
        I18nKey::DataConsistentTitle => "Daten konsistent",
        I18nKey::DataConsistentMsg => {
            "Die abgerufenen Metadaten sind identisch mit den lokalen Daten. Keine Zusammenführung erforderlich."
        }
        I18nKey::LiteratureMergedTitle => "Literatur zusammengeführt",
        I18nKey::LiteratureMergedMsg => {
            "Das Duplikat von \"{}\" ist identisch mit der Hauptliteratur und wurde in den Papierkorb verschoben."
        }

        // Fetch Error Tips
        I18nKey::FetchFailedArxiv => {
            "Diese Literatur hat keine ArXiv-ID oder verwandten Link. Metadaten können nicht von ArXiv abgerufen werden."
        }
        I18nKey::FetchFailedDblp => {
            "Der Titel der Literatur ist leer. Suche auf DBLP nicht möglich."
        }
        I18nKey::FetchFailedCrossref => {
            "Diese Literatur hat kein DOI-Feld oder es ist leer. Metadaten können nicht von Crossref abgerufen werden."
        }
        I18nKey::FetchFailedOpenAlex => {
            "DOI und Titel sind beide leer. Suche auf OpenAlex nicht möglich."
        }

        // Batch Update
        I18nKey::BatchUpdatingMetadata => "Batch-Metadaten-Update ({}/{})",

        // Settings - Translation
        I18nKey::TranslationSettings => "Übersetzungseinstellungen",
        I18nKey::TranslationSettingsDesc => {
            "Konfigurieren Sie die Übersetzungs-Engine und API-Schlüssel für den PDF-Reader."
        }
        I18nKey::TranslationEngine => "Übersetzungs-Engine",
        I18nKey::TranslationSettingsTab => "Übersetzung",
        I18nKey::NoApiKeyRequired => {
            "Diese Engine benötigt keinen API-Schlüssel und kann direkt verwendet werden."
        }
        I18nKey::AiBackend => "KI-Backend",
        I18nKey::NiuTransApiKey => "NiuTrans API-Schlüssel",
        I18nKey::GoogleApiKey => "Google Cloud API-Schlüssel",
        I18nKey::BaiduApiKey => "Baidu AppID#Key",
        I18nKey::YoudaoApiKey => "Youdao AppID#Key",
        I18nKey::DeepLApiKey => "DeepL API-Schlüssel",
        I18nKey::AiApiKey => "AI API-Schlüssel",
        I18nKey::AiApiBase => "API Basis-URL",
        I18nKey::AiModel => "Modell",
        I18nKey::AiContextWindow => "Kontextfenster",
        I18nKey::AiCompressionStrategy => "Kompressionsstrategie",
        I18nKey::SlidingWindow => "Schiebefenster",
        I18nKey::SummaryCompression => "Zusammenfassung",
        I18nKey::AiBackendName => "Name",
        I18nKey::AiBackendType => "Typ",
        I18nKey::AiAddBackend => "Backend hinzufügen",
        I18nKey::AiActive => "Aktiv",
        I18nKey::AiNoBackends => "Keine KI-Backends konfiguriert",
        I18nKey::TargetLanguage => "Zielsprache",
        I18nKey::EngineGoogleFree => "Google (kostenlos)",
        I18nKey::EngineBingFree => "Bing (kostenlos)",
        I18nKey::EngineGoogleCloud => "Google Cloud",
        I18nKey::EngineNiuTrans => "NiuTrans",
        I18nKey::EngineBaidu => "Baidu",
        I18nKey::EngineYoudao => "Youdao",
        I18nKey::EngineDeeplFree => "DeepL Free",
        I18nKey::EngineDeeplPro => "DeepL Pro",
        I18nKey::EngineAi => "KI",
        I18nKey::AiBackendsSettingsTab => "KI-Backends",
        I18nKey::AiChatSettingsTab => "KI-Chat",
        I18nKey::DefaultSystemPrompt => "Standard-Systemprompt",
        I18nKey::BackendSelection => "Backend-Auswahl",
        I18nKey::ActiveBackend => "Aktives Backend",
        I18nKey::EnableThinking => "Denken aktivieren",
        I18nKey::InternalReaderDesc => {
            "Wenn der externe Reader deaktiviert ist, wird die PDF mit dem integrierten Reader geöffnet"
        }
        // PDF View - Notes
        I18nKey::EditNotesMarkdown => "Notizen bearbeiten (Markdown)",

        // Feed
        // Bookmark
        I18nKey::UnnamedBookmark => "Unbenanntes Lesezeichen",
        I18nKey::SelectMacosPdfReader => "macOS PDF-Reader auswählen",
        I18nKey::SelectWindowsPdfReader => "Windows PDF-Reader auswählen",

        // Pdf Viewer
        I18nKey::NotePlaceholder => "Notizinhalt eingeben...",
        I18nKey::ViewNote => "Notiz anzeigen",
        I18nKey::AddNote => "Notiz hinzufügen",
        I18nKey::Highlight => "Hervorheben",
        I18nKey::Underline => "Unterstreichen",
        I18nKey::LoadingOutline => "Gliederung wird geladen...",
        I18nKey::NoOutline => "Dieses Dokument hat keine Gliederung",
        I18nKey::RectangleAnnotation => "Rechteck",
        I18nKey::PageRange => "Seite {}-{}",
        I18nKey::SinglePage => "Seite {}",
        I18nKey::SelectTextToTranslate => "Text zum Übersetzen auswählen",
        I18nKey::Translate => "Übersetzen",
        I18nKey::OriginalSection => "Original",
        I18nKey::TranslationSection => "Übersetzung",
        I18nKey::Translating => "Übersetzen...",
        I18nKey::TranslationPending => "Übersetzung ausstehend",
        I18nKey::NoNotes => "Keine Notizen",
        I18nKey::CopyAsImage => "Als Bild kopieren",
        I18nKey::PdfEngineError => "PDF-Rendering-Engine Fehler",
        I18nKey::CloseWindow => "Fenster schließen",
        I18nKey::TranslationNotImplemented => "Übersetzung nicht implementiert",
        I18nKey::CreatePip => "Neues PiP",
        I18nKey::DeletePage => "Seite löschen",
        I18nKey::SaveAsImage => "Als Bild speichern",

        // Pdf Viewer - AI Chat
        I18nKey::Chat => "AI-Chat",
        I18nKey::NewChat => "Neuer Chat",
        I18nKey::ChatInputPlaceholder => "Nachricht eingeben...",
        I18nKey::NoChatSessions => "Keine Chats",
        I18nKey::DeleteChat => "Chat löschen",
        I18nKey::SendSelection => "Auswahl senden",
        I18nKey::QuoteLabel => "Zitat",
        I18nKey::AttachFile => "Datei anhängen",
        I18nKey::NoAttachments => "Keine Anhänge",
        I18nKey::AiThinking => "Denke nach...",
        I18nKey::BackToSessions => "Zurück zur Liste",
        I18nKey::EditSystemPrompt => "System-Prompt bearbeiten",
        I18nKey::DefaultChatTitle => "Chat",
        I18nKey::ChatSessionDeleted => "Chat gelöscht",

        // PDF Search
        I18nKey::SearchInPdf => "PDF durchsuchen",
        I18nKey::SearchInputPlaceholder => "Suchbegriffe eingeben...",
        I18nKey::SyncMetadataTab => "Metadaten-Synchronisation",
        I18nKey::SyncAttachmentTab => "Anhang-Synchronisation",
        I18nKey::EnableGoogleDrive => "Google Drive aktivieren",
        I18nKey::GoogleDriveDesc => "Anhänge mit Google Drive synchronisieren",
        I18nKey::ClientId => "Client-ID",
        I18nKey::ClientSecret => "Client-Geheimnis",
        I18nKey::Authorize => "Autorisieren",
        I18nKey::DataManagement => "Data Management",
        I18nKey::ClearLocalDb => "Clear Local Database",
        I18nKey::ClearLocalFiles => "Clear Local Files",
        I18nKey::CheckLocalFiles => "Check Local Files",
        I18nKey::ClearCloudDb => "Clear Cloud Database",
        I18nKey::ClearCloudFiles => "Clear Cloud Files",
        I18nKey::PurgeSyncedDeletions => "Gelöschte Daten bereinigen",
        I18nKey::PurgeDeletedData => "Gelöschte Daten vollständig bereinigen (lokal + remote)",

        // File Library Sync Dialog
        I18nKey::FileLibraryInitRequiredTitle => "Remote-Dateispeicher initialisieren",
        I18nKey::FileLibraryInitRequiredDesc => {
            "Der Remote-Speicher ist leer und nicht initialisiert. Möchten Sie ihn als Anhangsspeicher für die aktuelle lokale Bibliothek initialisieren?"
        }
        I18nKey::FileLibraryInitializing => "Remote-Dateispeicher wird initialisiert...",
        I18nKey::FileLibraryInitSuccess => "Remote-Dateispeicher erfolgreich initialisiert",
        I18nKey::FileLibraryInitFailed => {
            "Initialisierung des Remote-Dateispeichers fehlgeschlagen"
        }
        I18nKey::FileLibraryUnidentified => {
            "Der Remote-Speicher enthält unbekannte Dateien ohne Identitätsnachweis; Synchronisation verweigert"
        }
        I18nKey::FileLibraryIdentityMismatch => {
            "Die ID des Remote-Dateispeichers stimmt nicht mit der lokalen Datenbank überein; Synchronisation verweigert"
        }

        // Attachment Instant Restore & Conflict Notifications
        I18nKey::AttachmentRestoring => {
            "Anhangsdatei wird aus dem Remote-Speicher wiederhergestellt..."
        }
        I18nKey::AttachmentPendingDownloadNotice => {
            "Anhang wartet auf On-Demand-Download; Sofortwiederherstellung wird versucht"
        }
        I18nKey::AttachmentUnrecoverableMissingNotice => {
            "Remote-Objekt existiert nicht; Datei kann nicht wiederhergestellt werden"
        }
        I18nKey::AttachmentFileConflictNotice => {
            "Lokale und Remote-Datei wurden geändert; automatisches Überschreiben verhindert"
        }
        I18nKey::AttachmentUnknownDivergenceNotice => {
            "Fehlende Baseline oder abweichender Inhalt; Überschreiben verhindert"
        }
        I18nKey::AttachmentRestoreFailedNotice => {
            "Wiederherstellung fehlgeschlagen; bitte Verbindung und Rechte prüfen"
        }
        I18nKey::AttachmentOpenFailedNotice => {
            "Öffnen des Anhangs mit dem externen Programm fehlgeschlagen"
        }
    }
}
