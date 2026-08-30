use crate::I18nKey;

pub fn translate(key: I18nKey) -> &'static str {
    match key {
        I18nKey::Library => "Bibliothèque",
        I18nKey::Subscription => "Abonnement",

        // Native macOS Menu
        I18nKey::Hide => "Masquer Lumen",
        I18nKey::HideOthers => "Masquer les autres",
        I18nKey::ShowAll => "Tout afficher",
        I18nKey::Services => "Services",
        I18nKey::AllLiterature => "Toute la littérature",
        I18nKey::Uncategorized => "Non classé",
        I18nKey::Trash => "Corbeille",
        I18nKey::Tags => "Tags",
        I18nKey::AllSubscription => "Toutes les abonnements",
        I18nKey::Unread => "Non lu",
        I18nKey::StatusReading => "En lecture",
        I18nKey::StatusRead => "Lu",
        I18nKey::FolderNamePlaceholder => "Nom du dossier",
        I18nKey::TagNamePlaceholder => "Nom du tag",
        I18nKey::Version => "Version",
        I18nKey::EmptyFolder => "Le dossier est vide",
        I18nKey::NoMatchFound => "Aucun résultat trouvé",
        I18nKey::SearchBoxPlaceholder => "Rechercher des articles, auteurs ou revues...",
        I18nKey::ManualAdd => "Ajouter manuellement",
        I18nKey::BibTeXImport => "Import BibTeX",
        I18nKey::DoiImport => "Import DOI",
        I18nKey::ArXivImport => "Import ArXiv",
        I18nKey::DblpSearch => "Recherche DBLP",
        I18nKey::DuplicateGroups => "Duplicate Groups",
        I18nKey::SyncConflicts => "Version Conflicts",
        I18nKey::NoDuplicatesFound => "No duplicates found",
        I18nKey::DuplicateSearch => "Rechercher les doublons",
        I18nKey::NewFolder => "Nouveau dossier",
        I18nKey::EmptyTrash => "Vider la corbeille",
        I18nKey::Rename => "Renommer",
        I18nKey::Delete => "Supprimer",
        I18nKey::NewSubscription => "Nouvel abonnement",
        I18nKey::OpenInBrowser => "Ouvrir dans le navigateur",
        I18nKey::MarkAsRead => "Marquer comme lu",
        I18nKey::MarkAsUnread => "Marquer comme non lu",
        I18nKey::UpdateSubscription => "Mettre à jour",
        I18nKey::EditSubscription => "Modifier l'abonnement",
        I18nKey::Unsubscribe => "Se désabonner",
        I18nKey::AddSubscription => "Ajouter un abonnement",
        I18nKey::UpdateAllSubscriptions => "Mettre à jour tous les abonnements",
        I18nKey::SubscriptionUpdated => "Abonnement {} mis à jour",
        I18nKey::SubscriptionUpdateFailed => "Échec de la mise à jour de {} : {}",
        I18nKey::NewSubFolder => "Nouveau sous-dossier",
        I18nKey::Edit => "Modifier",
        I18nKey::Quit => "Quitter",
        I18nKey::PermanentDelete => "Suppression permanente",
        I18nKey::CopyCitation => "Copier la citation",
        I18nKey::FetchFrom => "Récupérer via...",
        I18nKey::BatchFetchMetadata => "Mettre à jour les métadonnées en lot",
        I18nKey::AddTo => "Ajouter à...",
        I18nKey::RestoreTo => "Restaurer vers...",
        I18nKey::RemoveFromFolder => "Retirer du dossier",
        I18nKey::RevealInFinder => "Afficher dans le Finder",
        I18nKey::RevealInExplorer => "Afficher dans l'Explorateur",
        I18nKey::OpenPath => "Ouvrir le chemin",
        I18nKey::ReplaceFile => "Remplacer le fichier",
        I18nKey::DeleteFile => "Supprimer le fichier",
        I18nKey::Export => "Exporter",
        I18nKey::ExportAnnotatedPdf => "Exporter le PDF avec annotations",
        I18nKey::ExportAnnotatedPdfSuccess => "PDF exporté (avec annotations)",
        I18nKey::ExportAnnotatedPdfFailed => "Échec de l'export du PDF annoté",
        I18nKey::ExportAnnotatedPdfNoAnnotations => {
            "Ce document ne contient aucune annotation à exporter"
        }
        I18nKey::SelectNewFile => "Choisir un nouveau fichier",
        I18nKey::Confirm => "OK",
        I18nKey::LoadingMetadata => "Récupération des métadonnées...",
        I18nKey::FetchFailed => "Échec de la récupération",
        I18nKey::Retry => "Réessayer",
        I18nKey::Close => "Fermer",
        I18nKey::ConfirmFetch => "Récupérer",
        I18nKey::FetchFromSource => "Récupérer de {}",
        I18nKey::FetchPlaceholderDoi => "Entrez le DOI",
        I18nKey::FetchPlaceholderArxiv => "Entrez l'ID ArXiv",
        I18nKey::FetchPlaceholderBibtex => "Collez le BibTeX",
        I18nKey::FetchPlaceholderDblp => "Rechercher dans DBLP",
        I18nKey::FetchPlaceholderOpenAlex => "Rechercher dans OpenAlex",
        I18nKey::NoContentOrInvalidFormat => "Contenu vide ou format invalide",
        I18nKey::ImportFailed => "Échec de l'importation",

        I18nKey::LiteratureEditor => "Éditeur de littérature",
        I18nKey::AuthorPlaceholder => "Auteurs (séparés par des virgules)",
        I18nKey::JournalPlaceholder => "Revue / Conférence / Livre",
        I18nKey::Month => "Mois",
        I18nKey::Day => "Jour",
        I18nKey::Publisher => "Éditeur",
        I18nKey::Field => "Champ",
        I18nKey::LocalData => "Données locales",
        I18nKey::RemoteData => "Données distantes",
        I18nKey::SubscriptionEditor => "Éditeur d'abonnement",
        I18nKey::FeedName => "Nom",
        I18nKey::FeedUrl => "URL",
        I18nKey::UpdateInterval => "Intervalle de mise à jour",
        I18nKey::SubscriptionNamePlaceholder => "Nom",
        I18nKey::SubscriptionUrlPlaceholder => "URL RSS",
        I18nKey::UpdateIntervalPlaceholder => "Intervalle (heures)",
        I18nKey::Add => "Ajouter",

        I18nKey::SelectedSubscriptionCount => "{} abonnements sélectionnés",
        I18nKey::AddToLibrary => "+ Ajouter à la bibliothèque",
        I18nKey::NoSubscriptionSelected => "Aucun abonnement sélectionné",
        I18nKey::NoAbstract => "Aucun résumé disponible",

        I18nKey::CopyCitationTitle => "Copier la citation",
        I18nKey::Style => "Style",
        I18nKey::Preview => "Aperçu",
        I18nKey::NoLiteratureSelectedForCitation => "Aucune littérature sélectionnée",
        I18nKey::CitationError => "Échec de la génération",
        I18nKey::CitationBibTeX => "BibTeX",
        I18nKey::CitationIeee => "IEEE",
        I18nKey::CopiedToClipboard => "Copié dans le presse-papiers",
        I18nKey::CitationSettings => "Paramètres de citation",
        I18nKey::AbbreviateJournalInCitation => "Abréger le nom de la revue dans les citations",

        I18nKey::Type => "Type",
        I18nKey::TypeArticle => "Article de revue",
        I18nKey::TypeBook => "Livre",
        I18nKey::TypeConference => "Article de conférence",
        I18nKey::TypeThesis => "Thèse",
        I18nKey::TypePreprint => "Prépublication",
        I18nKey::TypeTechnicalReport => "Rapport technique",
        I18nKey::TypeWebpage => "Page web",
        I18nKey::TypeOther => "Autre",
        I18nKey::Title => "Titre",
        I18nKey::Authors => "Auteurs",
        I18nKey::Journal => "Revue",
        I18nKey::Year => "Année",
        I18nKey::Volume => "Vol.",
        I18nKey::Issue => "No.",
        I18nKey::Pages => "Pages",
        I18nKey::Url => "URL",
        I18nKey::Doi => "DOI",
        I18nKey::ArXiv => "ArXiv",
        I18nKey::Abstract => "Résumé",
        I18nKey::Notes => "Notes",
        I18nKey::Attachments => "Pièces jointes",
        I18nKey::Folders => "Dossiers",
        I18nKey::NoLiteratureSelected => "Aucune littérature sélectionnée",
        I18nKey::SelectedCount => "{} articles sélectionnés",
        I18nKey::MainFile => "Fichier principal",
        I18nKey::Attachment => "Pièce jointe",
        I18nKey::SetAsMainFile => "Définir comme principal",
        I18nKey::SetAsAttachment => "Définir comme pièce jointe",
        I18nKey::Expand => "Tout développer ↓",
        I18nKey::Collapse => "Réduire ↑",
        I18nKey::Publication => "Publication",
        I18nKey::PublicationAbbreviation => "Abréviation de la revue",
        I18nKey::RelatedLiterature => "Littérature connexe",
        I18nKey::AddCitation => "Ajouter une citation",
        I18nKey::References => "Références",
        I18nKey::CitedBy => "Cité par",
        I18nKey::Settings => "Paramètres",
        I18nKey::Language => "Langue",
        I18nKey::Appearance => "Apparence",
        I18nKey::UiScale => "Échelle de l'interface",
        I18nKey::LogLevel => "Niveau de journalisation",
        I18nKey::NotificationLevel => "Niveau de notification",
        I18nKey::ThemeStyle => "Style de thème",
        I18nKey::Theme => "Thème",
        I18nKey::Dark => "Sombre",
        I18nKey::Light => "Clair",
        I18nKey::System => "Système",
        I18nKey::General => "Général",
        I18nKey::Sync => "Synchronisation",
        I18nKey::About => "À propos",
        I18nKey::Cancel => "Annuler",
        I18nKey::Save => "Enregistrer",
        I18nKey::LibrarySettings => "Paramètres de la bibliothèque",
        I18nKey::AttachmentDir => "Répertoire des pièces jointes",
        I18nKey::AttachmentDirDesc => "Les PDF et pièces jointes seront enregistrés ici",
        I18nKey::DatabaseDir => "Répertoire de la base de données",
        I18nKey::DatabaseDirDesc => "Emplacement des fichiers de données",
        I18nKey::FilenameTemplate => "Format de nom de fichier",
        I18nKey::FilenameTemplateDesc => {
            "Custom renaming rules for attachments. Available variables: {title}, {author}, {year}, {publication}, {firstname}, {lastname}, {firstchartitle}. Supports using '/' for folder hierarchy."
        }
        I18nKey::GeneralOptions => "Options générales",
        I18nKey::CloudSyncDesc => "La synchronisation cloud est en cours de développement.",
        I18nKey::AboutDesc => "Gestionnaire de littérature haute performance construit avec GPUI.",
        I18nKey::BatchRename => "Batch Rename",
        I18nKey::BatchRenameCompleted => "Renommage groupé terminé : {} réussis, {} ignorés",
        I18nKey::BatchRenameCompletedWithFailures => {
            "Renommage groupé terminé : {} réussis, {} ignorés, {} échecs. {}"
        }
        I18nKey::BatchRenameFailed => "Impossible de poursuivre le renommage groupé : {}",
        I18nKey::BatchRenameSourceNotRegularFile => {
            "Le fichier source est absent ou n’est pas un fichier normal : {}"
        }
        I18nKey::BatchRenameSourceNameUnreadable => "Impossible de lire le nom du fichier source",
        I18nKey::BatchRenameTargetExists => "Le fichier cible existe déjà : {}",
        I18nKey::BatchRenameRenameFailed => "Échec du renommage du fichier : {}",
        I18nKey::BatchRenameSaveFailed => {
            "Les fichiers ont été renommés mais les enregistrements n’ont pas été sauvegardés ; impossible de continuer : {}"
        }
        I18nKey::CleanupOrphanedFiles => "Cleanup Orphaned Files",
        I18nKey::Copyright => "© 2026 Lumen. Tous droits réservés.",
        // Sort
        I18nKey::SortBy => "Trier",
        I18nKey::SortByTitle => "Titre",
        I18nKey::SortByAuthor => "Auteur",
        I18nKey::SortByYear => "Année",
        I18nKey::SortByJournal => "Journal",
        I18nKey::SortAscending => "Croissant",
        I18nKey::SortDescending => "Décroissant",

        I18nKey::UpdatedAt => "Mis à jour à",

        // Sync
        I18nKey::SyncMetadata => "Synchroniser les métadonnées",
        I18nKey::SyncAttachments => "Synchroniser les pièces jointes",
        I18nKey::TestConnection => "Tester la connexion",
        I18nKey::WebDavSettings => "Paramètres WebDAV",
        I18nKey::EnableWebDav => "Activer WebDAV",
        I18nKey::DatabaseSettings => "Paramètres de la base de données",
        I18nKey::EndpointUrl => "Adresse du serveur",
        I18nKey::Username => "Nom d'utilisateur",
        I18nKey::Password => "Mot de passe",
        I18nKey::RemotePath => "Chemin distant",
        I18nKey::Host => "Hôte",
        I18nKey::Port => "Port",
        I18nKey::DatabaseName => "Nom de la base de données",
        I18nKey::EnableSSL => "Activer SSL",
        I18nKey::UseRemoteDatabase => "Utiliser une base de données distante",
        I18nKey::ConnectionSuccess => "Connexion réussie",
        I18nKey::ConnectionFailed => "Échec de la connexion",
        I18nKey::SearchOrCreateTags => "Rechercher ou créer des tags...",
        I18nKey::CreateTag => "Créer \"{}\"",

        // PDF Viewer
        I18nKey::PdfViewerSettings => "Paramètres du lecteur PDF",
        I18nKey::PdfViewerSettingsDesc => {
            "Personnaliser l'application pour ouvrir les fichiers PDF"
        }
        I18nKey::UseCustomPdfViewer => "Utiliser un lecteur PDF personnalisé",
        I18nKey::PdfViewerPathMacos => "Application macOS",
        I18nKey::PdfViewerPathWindows => "Programme Windows",
        I18nKey::SelectMetadataCandidate => "Sélectionnez les métadonnées les plus correspondantes",

        I18nKey::NetworkProxySettings => "Network Proxy",
        I18nKey::EnableProxyServer => "Enable Custom Proxy Server",
        I18nKey::ProxyAddress => "Proxy Server Address",
        I18nKey::ProxyDesc => "Supports HTTP, HTTPS or SOCKS5 protocol, e.g. http://127.0.0.1:7890",

        // Service Errors
        // Error/Notification
        I18nKey::FileNotFoundTitle => "Fichier introuvable",
        I18nKey::FileNotFoundMsg => "Le chemin {:?} n'existe pas",
        I18nKey::DataConsistentTitle => "Données cohérentes",
        I18nKey::DataConsistentMsg => {
            "Les métadonnées récupérées sont identiques aux données locales. Aucune fusion nécessaire."
        }
        I18nKey::LiteratureMergedTitle => "Littérature fusionnée",
        I18nKey::LiteratureMergedMsg => {
            "Le doublon de \"{}\" est identique à la littérature principale et a été déplacé vers la corbeille."
        }

        // Fetch Error Tips
        I18nKey::FetchFailedArxiv => {
            "Cette littérature n'a pas d'ID ArXiv ou de lien associé. Impossible de récupérer les métadonnées depuis ArXiv."
        }
        I18nKey::FetchFailedDblp => {
            "Le titre de la littérature est vide. Impossible de rechercher sur DBLP."
        }
        I18nKey::FetchFailedCrossref => {
            "Cette littérature n'a pas de champ DOI ou il est vide. Impossible de récupérer les métadonnées depuis Crossref."
        }
        I18nKey::FetchFailedOpenAlex => {
            "Le DOI et le titre sont vides. Impossible de rechercher sur OpenAlex."
        }

        // Batch Update
        I18nKey::BatchUpdatingMetadata => "Mise à jour groupée des métadonnées ({}/{})",

        // Settings - Translation
        I18nKey::TranslationSettings => "Paramètres de traduction",
        I18nKey::TranslationSettingsDesc => {
            "Configurez le moteur de traduction et les clés API pour le lecteur PDF."
        }
        I18nKey::TranslationEngine => "Moteur de traduction",
        I18nKey::TranslationSettingsTab => "Traduction",
        I18nKey::NoApiKeyRequired => {
            "Ce moteur ne nécessite pas de clé API et peut être utilisé directement."
        }
        I18nKey::AiBackend => "Backend IA",
        I18nKey::NiuTransApiKey => "Clé API NiuTrans",
        I18nKey::GoogleApiKey => "Clé API Google Cloud",
        I18nKey::BaiduApiKey => "Baidu AppID#Key",
        I18nKey::YoudaoApiKey => "Youdao AppID#Key",
        I18nKey::DeepLApiKey => "Clé API DeepL",
        I18nKey::AiApiKey => "Clé API IA",
        I18nKey::AiApiBase => "URL de base API",
        I18nKey::AiModel => "Modèle",
        I18nKey::AiContextWindow => "Fenêtre de contexte",
        I18nKey::AiCompressionStrategy => "Stratégie de compression",
        I18nKey::SlidingWindow => "Fenêtre glissante",
        I18nKey::SummaryCompression => "Résumé",
        I18nKey::AiBackendName => "Nom",
        I18nKey::AiBackendType => "Type",
        I18nKey::AiAddBackend => "Ajouter un backend",
        I18nKey::AiActive => "Actif",
        I18nKey::AiNoBackends => "Aucun backend IA configuré",
        I18nKey::TargetLanguage => "Langue cible",
        I18nKey::EngineGoogleFree => "Google (gratuit)",
        I18nKey::EngineBingFree => "Bing (gratuit)",
        I18nKey::EngineGoogleCloud => "Google Cloud",
        I18nKey::EngineNiuTrans => "NiuTrans",
        I18nKey::EngineBaidu => "Baidu",
        I18nKey::EngineYoudao => "Youdao",
        I18nKey::EngineDeeplFree => "DeepL Free",
        I18nKey::EngineDeeplPro => "DeepL Pro",
        I18nKey::EngineAi => "IA",
        I18nKey::AiBackendsSettingsTab => "Backends IA",
        I18nKey::AiChatSettingsTab => "Chat IA",
        I18nKey::DefaultSystemPrompt => "Invite système par défaut",
        I18nKey::BackendSelection => "Sélection du backend",
        I18nKey::ActiveBackend => "Backend actif",
        I18nKey::EnableThinking => "Activer le raisonnement",
        I18nKey::InternalReaderDesc => {
            "Lorsque le lecteur externe est désactivé, le PDF sera ouvert avec le lecteur intégré"
        }
        // PDF View - Notes
        I18nKey::EditNotesMarkdown => "Modifier les notes (Markdown)",

        // Feed
        // Bookmark
        I18nKey::UnnamedBookmark => "Signet sans nom",
        I18nKey::SelectMacosPdfReader => "Choisir un lecteur PDF macOS",
        I18nKey::SelectWindowsPdfReader => "Choisir un lecteur PDF Windows",

        // Pdf Viewer
        I18nKey::NotePlaceholder => "Entrez le contenu de la note...",
        I18nKey::ViewNote => "Voir la note",
        I18nKey::AddNote => "Ajouter une note",
        I18nKey::Highlight => "Surligner",
        I18nKey::Underline => "Souligner",
        I18nKey::LoadingOutline => "Chargement du plan...",
        I18nKey::NoOutline => "Ce document n'a pas de plan",
        I18nKey::RectangleAnnotation => "Rectangle",
        I18nKey::PageRange => "Page {}-{}",
        I18nKey::SinglePage => "Page {}",
        I18nKey::SelectTextToTranslate => "Sélectionnez le texte à traduire",
        I18nKey::Translate => "Traduire",
        I18nKey::OriginalSection => "Original",
        I18nKey::TranslationSection => "Traduction",
        I18nKey::Translating => "Traduction en cours...",
        I18nKey::TranslationPending => "Traduction en attente",
        I18nKey::NoNotes => "Aucune note",
        I18nKey::CopyAsImage => "Copier en tant qu'image",
        I18nKey::PdfEngineError => "Erreur du moteur de rendu PDF",
        I18nKey::CloseWindow => "Fermer la fenêtre",
        I18nKey::TranslationNotImplemented => "Traduction non implémentée",
        I18nKey::CreatePip => "Nouveau PiP",
        I18nKey::DeletePage => "Supprimer la page",
        I18nKey::SaveAsImage => "Enregistrer en tant qu'image",

        // Pdf Viewer - AI Chat
        I18nKey::Chat => "Chat IA",
        I18nKey::NewChat => "Nouveau chat",
        I18nKey::ChatInputPlaceholder => "Saisissez un message...",
        I18nKey::NoChatSessions => "Aucune conversation",
        I18nKey::DeleteChat => "Supprimer le chat",
        I18nKey::SendSelection => "Envoyer la sélection",
        I18nKey::QuoteLabel => "Citation",
        I18nKey::AttachFile => "Joindre un fichier",
        I18nKey::NoAttachments => "Aucune pièce jointe",
        I18nKey::AiThinking => "Réflexion...",
        I18nKey::BackToSessions => "Retour aux sessions",
        I18nKey::EditSystemPrompt => "Modifier le prompt système",
        I18nKey::DefaultChatTitle => "Chat",
        I18nKey::ChatSessionDeleted => "Chat supprimé",

        // PDF Search
        I18nKey::SearchInPdf => "Rechercher dans le PDF",
        I18nKey::SearchInputPlaceholder => "Entrez des termes de recherche...",
        I18nKey::SyncMetadataTab => "Synchronisation des métadonnées",
        I18nKey::SyncAttachmentTab => "Synchronisation des pièces jointes",
        I18nKey::EnableGoogleDrive => "Activer Google Drive",
        I18nKey::GoogleDriveDesc => "Synchroniser les pièces jointes vers Google Drive",
        I18nKey::ClientId => "ID client",
        I18nKey::ClientSecret => "Secret client",
        I18nKey::Authorize => "Autoriser",
        I18nKey::DataManagement => "Data Management",
        I18nKey::ClearLocalDb => "Clear Local Database",
        I18nKey::ClearLocalFiles => "Clear Local Files",
        I18nKey::CheckLocalFiles => "Check Local Files",
        I18nKey::ClearCloudDb => "Clear Cloud Database",
        I18nKey::ClearCloudFiles => "Clear Cloud Files",
        I18nKey::PurgeSyncedDeletions => "Purger les données supprimées",
        I18nKey::PurgeDeletedData => "Purger les données supprimées (local + distant)",
    }
}
