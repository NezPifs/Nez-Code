use crate::bespoke_event_handling::apply_bespoke_event_handling;
use crate::command_exec::CommandExecManager;
use crate::command_exec::StartCommandExecParams;
use crate::config_manager::ConfigManager;
use crate::error_code::INPUT_TOO_LARGE_ERROR_CODE;
use crate::error_code::invalid_params;
use crate::models::supported_models;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::RequestContext;
use crate::outgoing_message::ThreadScopedOutgoingMessageSender;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_status::ThreadWatchManager;
use crate::thread_status::resolve_thread_status;
use chrono::Duration as ChronoDuration;
use chrono::SecondsFormat;
use nez_analytics::AnalyticsEventsClient;
use nez_analytics::AnalyticsJsonRpcError;
use nez_analytics::InputError;
use nez_analytics::TurnSteerRequestError;
use nez_app_server_protocol::Account;
use nez_app_server_protocol::AccountLoginCompletedNotification;
use nez_app_server_protocol::AccountTokenUsageDailyBucket;
use nez_app_server_protocol::AccountTokenUsageSummary;
use nez_app_server_protocol::AccountUpdatedNotification;
use nez_app_server_protocol::AddCreditsNudgeCreditType;
use nez_app_server_protocol::AddCreditsNudgeEmailStatus;
use nez_app_server_protocol::AdditionalContextEntry;
use nez_app_server_protocol::AdditionalContextKind;
use nez_app_server_protocol::AppListUpdatedNotification;
use nez_app_server_protocol::AppSummary;
use nez_app_server_protocol::AppTemplateSummary;
use nez_app_server_protocol::AppTemplateUnavailableReason;
use nez_app_server_protocol::AppsInstalledParams;
use nez_app_server_protocol::AppsInstalledResponse;
use nez_app_server_protocol::AppsListParams;
use nez_app_server_protocol::AppsListResponse;
use nez_app_server_protocol::AppsReadParams;
use nez_app_server_protocol::AppsReadResponse;
use nez_app_server_protocol::AskForApproval;
use nez_app_server_protocol::AuthMode;
use nez_app_server_protocol::CancelLoginAccountParams;
use nez_app_server_protocol::CancelLoginAccountResponse;
use nez_app_server_protocol::CancelLoginAccountStatus;
use nez_app_server_protocol::ClientInfo;
use nez_app_server_protocol::ClientRequest;
use nez_app_server_protocol::ClientResponsePayload;
use nez_app_server_protocol::CollaborationModeListParams;
use nez_app_server_protocol::CollaborationModeListResponse;
use nez_app_server_protocol::CommandExecParams;
use nez_app_server_protocol::CommandExecResizeParams;
use nez_app_server_protocol::CommandExecTerminateParams;
use nez_app_server_protocol::CommandExecWriteParams;
use nez_app_server_protocol::ConfigWarningNotification;
use nez_app_server_protocol::ConsumeAccountRateLimitResetCreditOutcome;
use nez_app_server_protocol::ConsumeAccountRateLimitResetCreditParams;
use nez_app_server_protocol::ConsumeAccountRateLimitResetCreditResponse;
use nez_app_server_protocol::ConversationGitInfo;
use nez_app_server_protocol::ConversationSummary;
use nez_app_server_protocol::DeprecationNoticeNotification;
use nez_app_server_protocol::DynamicToolFunctionSpec;
use nez_app_server_protocol::DynamicToolNamespaceTool;
use nez_app_server_protocol::DynamicToolSpec;
use nez_app_server_protocol::EnvironmentAddParams;
use nez_app_server_protocol::EnvironmentAddResponse;
use nez_app_server_protocol::EnvironmentInfoParams;
use nez_app_server_protocol::EnvironmentInfoResponse;
use nez_app_server_protocol::EnvironmentShellInfo;
use nez_app_server_protocol::EnvironmentStatusKind;
use nez_app_server_protocol::EnvironmentStatusParams;
use nez_app_server_protocol::EnvironmentStatusResponse;
use nez_app_server_protocol::ExperimentalFeature as ApiExperimentalFeature;
use nez_app_server_protocol::ExperimentalFeatureListParams;
use nez_app_server_protocol::ExperimentalFeatureListResponse;
use nez_app_server_protocol::ExperimentalFeatureStage as ApiExperimentalFeatureStage;
use nez_app_server_protocol::FeedbackUploadParams;
use nez_app_server_protocol::FeedbackUploadResponse;
use nez_app_server_protocol::GetAccountParams;
use nez_app_server_protocol::GetAccountRateLimitsResponse;
use nez_app_server_protocol::GetAccountResponse;
use nez_app_server_protocol::GetAccountTokenUsageParams;
use nez_app_server_protocol::GetAccountTokenUsageResponse;
use nez_app_server_protocol::GetAuthStatusParams;
use nez_app_server_protocol::GetAuthStatusResponse;
use nez_app_server_protocol::GetConversationSummaryParams;
use nez_app_server_protocol::GetConversationSummaryResponse;
use nez_app_server_protocol::GetWorkspaceMessagesResponse;
use nez_app_server_protocol::GitDiffToRemoteParams;
use nez_app_server_protocol::GitDiffToRemoteResponse;
use nez_app_server_protocol::GitInfo as ApiGitInfo;
use nez_app_server_protocol::HookHandlerMetadata;
use nez_app_server_protocol::HookMetadata;
use nez_app_server_protocol::HooksListParams;
use nez_app_server_protocol::HooksListResponse;
use nez_app_server_protocol::InitializeParams;
use nez_app_server_protocol::InitializeResponse;
use nez_app_server_protocol::InstalledApp;
use nez_app_server_protocol::JSONRPCErrorError;
use nez_app_server_protocol::ListMcpServerStatusParams;
use nez_app_server_protocol::ListMcpServerStatusResponse;
use nez_app_server_protocol::LoginAccountParams;
use nez_app_server_protocol::LoginAccountResponse;
use nez_app_server_protocol::LoginApiKeyParams;
use nez_app_server_protocol::LoginAppBrand;
use nez_app_server_protocol::LogoutAccountResponse;
use nez_app_server_protocol::MarketplaceAddParams;
use nez_app_server_protocol::MarketplaceAddResponse;
use nez_app_server_protocol::MarketplaceInterface;
use nez_app_server_protocol::MarketplaceRemoveParams;
use nez_app_server_protocol::MarketplaceRemoveResponse;
use nez_app_server_protocol::MarketplaceUpgradeErrorInfo;
use nez_app_server_protocol::MarketplaceUpgradeParams;
use nez_app_server_protocol::MarketplaceUpgradeResponse;
use nez_app_server_protocol::McpResourceReadParams;
use nez_app_server_protocol::McpResourceReadResponse;
use nez_app_server_protocol::McpServerOauthClientRegistration;
use nez_app_server_protocol::McpServerOauthLoginCompletedNotification;
use nez_app_server_protocol::McpServerOauthLoginParams;
use nez_app_server_protocol::McpServerOauthLoginResponse;
use nez_app_server_protocol::McpServerRefreshResponse;
use nez_app_server_protocol::McpServerStatus;
use nez_app_server_protocol::McpServerStatusDetail;
use nez_app_server_protocol::McpServerToolCallParams;
use nez_app_server_protocol::McpServerToolCallResponse;
use nez_app_server_protocol::MemoryResetResponse;
use nez_app_server_protocol::MockExperimentalMethodParams;
use nez_app_server_protocol::MockExperimentalMethodResponse;
use nez_app_server_protocol::ModelListParams;
use nez_app_server_protocol::ModelListResponse;
use nez_app_server_protocol::NezErrorInfo;
use nez_app_server_protocol::PermissionProfileListParams;
use nez_app_server_protocol::PermissionProfileListResponse;
use nez_app_server_protocol::PermissionProfileSummary;
use nez_app_server_protocol::PluginDetail;
use nez_app_server_protocol::PluginInstallParams;
use nez_app_server_protocol::PluginInstallResponse;
use nez_app_server_protocol::PluginInstalledParams;
use nez_app_server_protocol::PluginInstalledResponse;
use nez_app_server_protocol::PluginInterface;
use nez_app_server_protocol::PluginListMarketplaceKind;
use nez_app_server_protocol::PluginListParams;
use nez_app_server_protocol::PluginListResponse;
use nez_app_server_protocol::PluginMarketplaceEntry;
use nez_app_server_protocol::PluginReadParams;
use nez_app_server_protocol::PluginReadResponse;
use nez_app_server_protocol::PluginShareCheckoutParams;
use nez_app_server_protocol::PluginShareCheckoutResponse;
use nez_app_server_protocol::PluginShareContext;
use nez_app_server_protocol::PluginShareDeleteParams;
use nez_app_server_protocol::PluginShareDeleteResponse;
use nez_app_server_protocol::PluginShareDiscoverability;
use nez_app_server_protocol::PluginShareListItem;
use nez_app_server_protocol::PluginShareListParams;
use nez_app_server_protocol::PluginShareListResponse;
use nez_app_server_protocol::PluginSharePrincipal;
use nez_app_server_protocol::PluginSharePrincipalType;
use nez_app_server_protocol::PluginShareSaveParams;
use nez_app_server_protocol::PluginShareSaveResponse;
use nez_app_server_protocol::PluginShareTarget;
use nez_app_server_protocol::PluginShareUpdateDiscoverability;
use nez_app_server_protocol::PluginShareUpdateTargetsParams;
use nez_app_server_protocol::PluginShareUpdateTargetsResponse;
use nez_app_server_protocol::PluginSkillReadParams;
use nez_app_server_protocol::PluginSkillReadResponse;
use nez_app_server_protocol::PluginSource;
use nez_app_server_protocol::PluginSummary;
use nez_app_server_protocol::PluginUninstallParams;
use nez_app_server_protocol::PluginUninstallResponse;
use nez_app_server_protocol::RateLimitResetCredit;
use nez_app_server_protocol::RateLimitResetCreditStatus;
use nez_app_server_protocol::RateLimitResetCreditsSummary;
use nez_app_server_protocol::RateLimitResetType;
use nez_app_server_protocol::RequestId;
use nez_app_server_protocol::ReviewDelivery as ApiReviewDelivery;
use nez_app_server_protocol::ReviewStartParams;
use nez_app_server_protocol::ReviewStartResponse;
use nez_app_server_protocol::ReviewTarget as ApiReviewTarget;
use nez_app_server_protocol::SandboxMode;
use nez_app_server_protocol::SendAddCreditsNudgeEmailParams;
use nez_app_server_protocol::SendAddCreditsNudgeEmailResponse;
use nez_app_server_protocol::ServerNotification;
use nez_app_server_protocol::ServerRequestResolvedNotification;
use nez_app_server_protocol::SkillSummary;
use nez_app_server_protocol::SkillsConfigWriteParams;
use nez_app_server_protocol::SkillsConfigWriteResponse;
use nez_app_server_protocol::SkillsExtraRootsSetParams;
use nez_app_server_protocol::SkillsExtraRootsSetResponse;
use nez_app_server_protocol::SkillsListParams;
use nez_app_server_protocol::SkillsListResponse;
use nez_app_server_protocol::SortDirection;
use nez_app_server_protocol::Thread;
use nez_app_server_protocol::ThreadApproveGuardianDeniedActionParams;
use nez_app_server_protocol::ThreadApproveGuardianDeniedActionResponse;
use nez_app_server_protocol::ThreadArchiveParams;
use nez_app_server_protocol::ThreadArchiveResponse;
use nez_app_server_protocol::ThreadArchivedNotification;
use nez_app_server_protocol::ThreadBackgroundTerminal;
use nez_app_server_protocol::ThreadBackgroundTerminalsCleanParams;
use nez_app_server_protocol::ThreadBackgroundTerminalsCleanResponse;
use nez_app_server_protocol::ThreadBackgroundTerminalsListParams;
use nez_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use nez_app_server_protocol::ThreadBackgroundTerminalsTerminateParams;
use nez_app_server_protocol::ThreadBackgroundTerminalsTerminateResponse;
use nez_app_server_protocol::ThreadClosedNotification;
use nez_app_server_protocol::ThreadCompactStartParams;
use nez_app_server_protocol::ThreadCompactStartResponse;
use nez_app_server_protocol::ThreadDecrementElicitationParams;
use nez_app_server_protocol::ThreadDecrementElicitationResponse;
use nez_app_server_protocol::ThreadDeleteParams;
use nez_app_server_protocol::ThreadDeleteResponse;
use nez_app_server_protocol::ThreadDeletedNotification;
use nez_app_server_protocol::ThreadForkParams;
use nez_app_server_protocol::ThreadForkResponse;
use nez_app_server_protocol::ThreadGoal;
use nez_app_server_protocol::ThreadGoalClearParams;
use nez_app_server_protocol::ThreadGoalClearResponse;
use nez_app_server_protocol::ThreadGoalClearedNotification;
use nez_app_server_protocol::ThreadGoalGetParams;
use nez_app_server_protocol::ThreadGoalGetResponse;
use nez_app_server_protocol::ThreadGoalSetParams;
use nez_app_server_protocol::ThreadGoalSetResponse;
use nez_app_server_protocol::ThreadGoalStatus;
use nez_app_server_protocol::ThreadGoalUpdatedNotification;
use nez_app_server_protocol::ThreadHistoryBuilder;
#[cfg(test)]
use nez_app_server_protocol::ThreadHistoryMode;
use nez_app_server_protocol::ThreadIncrementElicitationParams;
use nez_app_server_protocol::ThreadIncrementElicitationResponse;
use nez_app_server_protocol::ThreadInjectItemsParams;
use nez_app_server_protocol::ThreadInjectItemsResponse;
use nez_app_server_protocol::ThreadItem;
use nez_app_server_protocol::ThreadItemEntry;
use nez_app_server_protocol::ThreadItemsListParams;
use nez_app_server_protocol::ThreadItemsListResponse;
use nez_app_server_protocol::ThreadItemsReadParams;
use nez_app_server_protocol::ThreadItemsReadResponse;
use nez_app_server_protocol::ThreadListCwdFilter;
use nez_app_server_protocol::ThreadListParams;
use nez_app_server_protocol::ThreadListResponse;
use nez_app_server_protocol::ThreadLoadedListParams;
use nez_app_server_protocol::ThreadLoadedListResponse;
use nez_app_server_protocol::ThreadMemoryModeSetParams;
use nez_app_server_protocol::ThreadMemoryModeSetResponse;
use nez_app_server_protocol::ThreadMetadataGitInfoUpdateParams;
use nez_app_server_protocol::ThreadMetadataUpdateParams;
use nez_app_server_protocol::ThreadMetadataUpdateResponse;
use nez_app_server_protocol::ThreadNameUpdatedNotification;
use nez_app_server_protocol::ThreadProjectUpdatedNotification;
use nez_app_server_protocol::ThreadReadParams;
use nez_app_server_protocol::ThreadReadResponse;
use nez_app_server_protocol::ThreadRealtimeAppendAudioParams;
use nez_app_server_protocol::ThreadRealtimeAppendAudioResponse;
use nez_app_server_protocol::ThreadRealtimeAppendSpeechParams;
use nez_app_server_protocol::ThreadRealtimeAppendSpeechResponse;
use nez_app_server_protocol::ThreadRealtimeAppendTextParams;
use nez_app_server_protocol::ThreadRealtimeAppendTextResponse;
use nez_app_server_protocol::ThreadRealtimeListVoicesResponse;
use nez_app_server_protocol::ThreadRealtimeStartParams;
use nez_app_server_protocol::ThreadRealtimeStartResponse;
use nez_app_server_protocol::ThreadRealtimeStartTransport;
use nez_app_server_protocol::ThreadRealtimeStopParams;
use nez_app_server_protocol::ThreadRealtimeStopResponse;
use nez_app_server_protocol::ThreadResumeInitialTurnsPageParams;
use nez_app_server_protocol::ThreadResumeParams;
use nez_app_server_protocol::ThreadResumeResponse;
use nez_app_server_protocol::ThreadSearchOccurrence;
use nez_app_server_protocol::ThreadSearchOccurrencesParams;
use nez_app_server_protocol::ThreadSearchOccurrencesResponse;
use nez_app_server_protocol::ThreadSearchParams;
use nez_app_server_protocol::ThreadSearchResponse;
use nez_app_server_protocol::ThreadSearchResult;
use nez_app_server_protocol::ThreadSearchSortKey;
use nez_app_server_protocol::ThreadSearchTextRange;
use nez_app_server_protocol::ThreadSetNameParams;
use nez_app_server_protocol::ThreadSetNameResponse;
use nez_app_server_protocol::ThreadSettings;
use nez_app_server_protocol::ThreadSettingsUpdateParams;
use nez_app_server_protocol::ThreadSettingsUpdateResponse;
use nez_app_server_protocol::ThreadShellCommandParams;
use nez_app_server_protocol::ThreadShellCommandResponse;
use nez_app_server_protocol::ThreadSortKey;
use nez_app_server_protocol::ThreadSourceKind;
use nez_app_server_protocol::ThreadStartParams;
use nez_app_server_protocol::ThreadStartResponse;
use nez_app_server_protocol::ThreadStartedNotification;
use nez_app_server_protocol::ThreadStatus;
use nez_app_server_protocol::ThreadTimelineListParams;
use nez_app_server_protocol::ThreadTimelineListResponse;
use nez_app_server_protocol::ThreadTurnsListParams;
use nez_app_server_protocol::ThreadTurnsListResponse;
use nez_app_server_protocol::ThreadUnarchiveParams;
use nez_app_server_protocol::ThreadUnarchiveResponse;
use nez_app_server_protocol::ThreadUnarchivedNotification;
use nez_app_server_protocol::ThreadUnsubscribeParams;
use nez_app_server_protocol::ThreadUnsubscribeResponse;
use nez_app_server_protocol::ThreadUnsubscribeStatus;
use nez_app_server_protocol::Turn;
use nez_app_server_protocol::TurnEnvironmentParams;
use nez_app_server_protocol::TurnError;
use nez_app_server_protocol::TurnInterruptParams;
use nez_app_server_protocol::TurnInterruptResponse;
use nez_app_server_protocol::TurnItemsView;
use nez_app_server_protocol::TurnSettingsUpdateParams;
use nez_app_server_protocol::TurnSettingsUpdateResponse;
use nez_app_server_protocol::TurnSettingsUpdateStatus;
use nez_app_server_protocol::TurnStartParams;
use nez_app_server_protocol::TurnStartResponse;
use nez_app_server_protocol::TurnStatus;
use nez_app_server_protocol::TurnSteerParams;
use nez_app_server_protocol::TurnSteerResponse;
use nez_app_server_protocol::UserInput as V2UserInput;
use nez_app_server_protocol::WindowsSandboxReadiness;
use nez_app_server_protocol::WindowsSandboxReadinessResponse;
use nez_app_server_protocol::WindowsSandboxSetupCompletedNotification;
use nez_app_server_protocol::WindowsSandboxSetupMode;
use nez_app_server_protocol::WindowsSandboxSetupStartParams;
use nez_app_server_protocol::WindowsSandboxSetupStartResponse;
use nez_app_server_protocol::WorkspaceMessage;
use nez_app_server_protocol::WorkspaceMessageType;
use nez_arg0::Arg0DispatchPaths;
use nez_backend_client::AddCreditsNudgeCreditType as BackendAddCreditsNudgeCreditType;
use nez_backend_client::Client as BackendClient;
use nez_backend_client::ConsumeRateLimitResetCreditCode as BackendConsumeRateLimitResetCreditCode;
use nez_backend_client::NezWorkspaceMessage as BackendWorkspaceMessage;
use nez_backend_client::NezWorkspaceMessageType as BackendWorkspaceMessageType;
use nez_backend_client::NezWorkspaceMessagesResponse as BackendWorkspaceMessagesResponse;
use nez_backend_client::RateLimitResetCreditDetails as BackendRateLimitResetCreditDetails;
use nez_backend_client::RateLimitResetCreditsDetails as BackendRateLimitResetCreditsDetails;
use nez_backend_client::RequestError as BackendRequestError;
use nez_backend_client::TokenUsageProfile;
use nez_chatgpt::connectors;
use nez_config::CloudConfigBundleLoadError;
use nez_config::CloudConfigBundleLoadErrorCode;
use nez_config::ConfigLayerStack;
use nez_config::loader::project_trust_key;
use nez_config::types::McpServerTransportConfig;
use nez_connectors::AppInfo;
use nez_core::ForkSnapshot;
use nez_core::McpManager;
use nez_core::NewThread;
use nez_core::NezThread;
use nez_core::NezThreadSettingsOverrides;
use nez_core::NotSubmittedReason;
#[cfg(test)]
use nez_core::SessionMeta;
use nez_core::StartThreadOptions;
use nez_core::SteerSubmission;
use nez_core::ThreadConfigSnapshot;
use nez_core::ThreadManager;
use nez_core::TurnInput;
use nez_core::TurnInputRequest;
use nez_core::TurnInputSubmission;
use nez_core::TurnStartOptions;
use nez_core::config::Config;
use nez_core::config::ConfigOverrides;
use nez_core::config::NetworkProxyAuditMetadata;
use nez_core::config::edit::ConfigEdit;
use nez_core::config::edit::ConfigEditsBuilder;
use nez_core::connectors::AccessibleConnectorsStatus;
use nez_core::exec::ExecCapturePolicy;
use nez_core::exec::ExecExpiration;
use nez_core::exec::ExecParams;
use nez_core::exec_env::create_env;
use nez_core::path_utils;
#[cfg(test)]
use nez_core::read_head_for_summary;
use nez_core::sandboxing::SandboxPermissions;
use nez_core::truncate_rollout_after_turn_id;
use nez_core::truncate_rollout_before_turn_id;
use nez_core::validate_environment_ids_and_cwds;
use nez_core::windows_sandbox::WindowsSandboxLevelExt;
use nez_core::windows_sandbox::WindowsSandboxSetupMode as CoreWindowsSandboxSetupMode;
use nez_core::windows_sandbox::WindowsSandboxSetupRequest;
use nez_core::windows_sandbox::sandbox_setup_is_complete;
use nez_core_plugins::PluginInstallError as CorePluginInstallError;
use nez_core_plugins::PluginInstallRequest;
use nez_core_plugins::PluginReadRequest;
use nez_core_plugins::PluginUninstallError as CorePluginUninstallError;
use nez_core_plugins::PluginsManager;
use nez_core_plugins::loader::load_plugin_apps;
use nez_core_plugins::manifest::PluginManifestInterface;
use nez_core_plugins::marketplace::MarketplaceError;
use nez_core_plugins::marketplace::MarketplacePluginSource;
use nez_core_plugins::marketplace_add::MarketplaceAddError;
use nez_core_plugins::marketplace_add::MarketplaceAddRequest;
use nez_core_plugins::marketplace_add::add_marketplace as add_marketplace_to_nez_home;
use nez_core_plugins::marketplace_remove::MarketplaceRemoveError;
use nez_core_plugins::marketplace_remove::MarketplaceRemoveRequest as CoreMarketplaceRemoveRequest;
use nez_core_plugins::marketplace_remove::remove_marketplace;
use nez_core_plugins::remote::RemoteMarketplace;
use nez_core_plugins::remote::RemoteMarketplaceSource;
use nez_core_plugins::remote::RemotePluginCatalogError;
use nez_core_plugins::remote::RemotePluginDetail as RemoteCatalogPluginDetail;
use nez_core_plugins::remote::RemotePluginServiceConfig;
use nez_core_plugins::remote::RemotePluginShareContext as RemoteCatalogPluginShareContext;
use nez_core_plugins::remote::RemotePluginShareSummary as RemoteCatalogPluginShareSummary;
use nez_core_plugins::remote::RemotePluginSummary as RemoteCatalogPluginSummary;
use nez_exec_server::EnvironmentManager;
use nez_exec_server::EnvironmentObservedStatus;
use nez_exec_server::LOCAL_ENVIRONMENT_ID;
use nez_exec_server::LOCAL_FS;
use nez_features::FEATURES;
use nez_features::Feature;
use nez_features::Stage;
use nez_feedback::FeedbackAttachmentPath;
use nez_feedback::FeedbackUploadOptions;
use nez_feedback::NezFeedback;
use nez_git_utils::git_diff_to_remote;
use nez_git_utils::resolve_root_git_project_for_trust;
use nez_login::AuthManager;
use nez_login::LoginSuccessPage;
use nez_login::LoginSuccessPageBrand;
use nez_login::NEZ_OPEN_APP_URL;
use nez_login::NezAuth;
use nez_login::ServerOptions as LoginServerOptions;
use nez_login::ShutdownHandle;
use nez_login::complete_device_code_login;
use nez_login::login_with_api_key;
use nez_login::login_with_bedrock_api_key;
use nez_login::oauth_client_id;
use nez_login::request_device_code;
use nez_login::run_login_server;
use nez_mcp::McpRuntimeContext;
use nez_mcp::McpServerStatusSnapshot;
use nez_mcp::McpSnapshotDetail;
use nez_mcp::collect_mcp_server_status_snapshot_with_detail;
use nez_mcp::discover_supported_scopes;
use nez_mcp::read_mcp_resource as read_mcp_resource_without_thread;
use nez_mcp::resolve_oauth_scopes;
use nez_memories_write::clear_memory_roots_contents;
use nez_model_provider::create_model_provider;
use nez_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets;
use nez_protocol::ThreadId;
use nez_protocol::config_types::CollaborationMode;
use nez_protocol::config_types::ForcedLoginMethod;
use nez_protocol::config_types::Personality;
use nez_protocol::config_types::ReasoningSummary;
use nez_protocol::config_types::TrustLevel;
use nez_protocol::config_types::WindowsSandboxLevel;
use nez_protocol::error::NezErr;
use nez_protocol::error::Result as NezResult;
#[cfg(test)]
use nez_protocol::items::TurnItem;
use nez_protocol::models::ResponseItem;
use nez_protocol::openai_models::ReasoningEffort;
use nez_protocol::protocol::AgentStatus;
use nez_protocol::protocol::ConversationAudioParams;
use nez_protocol::protocol::ConversationSpeechParams;
use nez_protocol::protocol::ConversationStartParams;
use nez_protocol::protocol::ConversationStartTransport;
use nez_protocol::protocol::ConversationTextParams;
use nez_protocol::protocol::EnvironmentConfigState;
use nez_protocol::protocol::EventMsg;
#[cfg(test)]
use nez_protocol::protocol::GitInfo as CoreGitInfo;
use nez_protocol::protocol::McpAuthStatus as CoreMcpAuthStatus;
use nez_protocol::protocol::Op;
use nez_protocol::protocol::RealtimeVoicesList;
use nez_protocol::protocol::ReviewDelivery as CoreReviewDelivery;
use nez_protocol::protocol::ReviewRequest;
use nez_protocol::protocol::ReviewTarget as CoreReviewTarget;
use nez_protocol::protocol::SessionConfiguredEvent;
#[cfg(test)]
use nez_protocol::protocol::SessionMetaLine;
use nez_protocol::protocol::TurnEnvironmentRequest;
use nez_protocol::protocol::W3cTraceContext;
use nez_protocol::protocol::strip_user_message_prefix;
use nez_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use nez_protocol::user_input::UserInput as CoreInputItem;
use nez_rmcp_client::McpOAuthClientRegistration;
use nez_rmcp_client::StreamableHttpRedirectMode;
use nez_rmcp_client::perform_oauth_login_return_url;
use nez_rollout::InitialHistory;
use nez_rollout::ResumedHistory;
use nez_rollout::RolloutItem;
use nez_rollout::persisted_rollout_item;
use nez_rollout::state_db::StateDbHandle;
use nez_rollout::state_db::reconcile_rollout;
use nez_state::ThreadMetadata;
use nez_state::log_db::LogDbLayer;
use nez_thread_store::ArchiveThreadParams as StoreArchiveThreadParams;
use nez_thread_store::ArchiveThreadsParams as StoreArchiveThreadsParams;
use nez_thread_store::ClearableField as StoreClearableField;
use nez_thread_store::DeleteThreadsParams as StoreDeleteThreadsParams;
use nez_thread_store::GitInfoPatch as StoreGitInfoPatch;
use nez_thread_store::ItemSortKey as StoreItemSortKey;
use nez_thread_store::ListItemsParams as StoreListItemsParams;
use nez_thread_store::ListItemsPosition as StoreListItemsPosition;
use nez_thread_store::ListThreadsParams as StoreListThreadsParams;
use nez_thread_store::ListTimelineParams as StoreListTimelineParams;
use nez_thread_store::ListTurnsParams as StoreListTurnsParams;
use nez_thread_store::LoadThreadHistoryParams as StoreLoadThreadHistoryParams;
use nez_thread_store::LocalThreadStore;
use nez_thread_store::ReadThreadByRolloutPathParams as StoreReadThreadByRolloutPathParams;
use nez_thread_store::ReadThreadParams as StoreReadThreadParams;
use nez_thread_store::SearchThreadOccurrencesParams as StoreSearchThreadOccurrencesParams;
use nez_thread_store::SearchThreadsParams as StoreSearchThreadsParams;
use nez_thread_store::SortDirection as StoreSortDirection;
use nez_thread_store::StoredThread;
use nez_thread_store::StoredTurn;
use nez_thread_store::StoredTurnItemsView;
use nez_thread_store::StoredTurnStatus;
use nez_thread_store::ThreadMetadataPatch as StoreThreadMetadataPatch;
use nez_thread_store::ThreadRelationFilter as StoreThreadRelationFilter;
use nez_thread_store::ThreadSortKey as StoreThreadSortKey;
use nez_thread_store::ThreadStore;
use nez_thread_store::ThreadStoreError;
use nez_utils_absolute_path::AbsolutePathBuf;
use nez_utils_pty::DEFAULT_OUTPUT_BYTES_CAP;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;
use tokio_util::task::TaskTracker;
use toml::Value as TomlValue;
use tracing::Instrument;
use tracing::error;
use tracing::info;
use tracing::warn;
use uuid::Uuid;

#[cfg(test)]
use nez_app_server_protocol::ServerRequest;

mod account_processor;
mod apps_processor;
mod bedrock_auth;
mod catalog_processor;
mod command_exec_processor;
mod config_processor;
#[cfg(test)]
#[path = "request_processors/config_reload_tests.rs"]
mod config_reload_tests;
mod diagnostics;
mod environment_processor;
mod feedback_doctor_report;
mod feedback_processor;
mod feedback_rollout_history;
mod feedback_thread_index;
mod fs_processor;
mod git_processor;
mod initialize_processor;
mod marketplace_processor;
mod mcp_event_stream;
mod mcp_processor;
mod memory_status;
mod persisted_resume_settings;
mod plugins;
mod process_exec_processor;
mod projects;
mod remote_control_processor;
mod rollout;
mod search;
mod thread_attachments;
mod thread_enrichment;
mod thread_fork_goal;
mod thread_input;
mod thread_processor;
mod thread_queue_processor;
mod thread_read_state;
mod thread_sections;
mod token_usage_replay;
mod turn_processor;
mod windows_sandbox_processor;

pub(crate) use account_processor::AccountRequestProcessor;
pub(crate) use apps_processor::AppsRequestProcessor;
pub(crate) use catalog_processor::CatalogRequestProcessor;
pub(crate) use command_exec_processor::CommandExecRequestProcessor;
pub(crate) use config_processor::ConfigRequestProcessor;
pub(crate) use diagnostics::read_server_diagnostics;
pub(crate) use environment_processor::EnvironmentRequestProcessor;
pub(crate) use feedback_processor::FeedbackRequestProcessor;
pub(crate) use fs_processor::FsRequestProcessor;
pub(crate) use git_processor::GitRequestProcessor;
pub(crate) use initialize_processor::InitializeRequestProcessor;
pub(crate) use marketplace_processor::MarketplaceRequestProcessor;
pub(crate) use mcp_event_stream::McpEventStreamReady;
pub(crate) use mcp_event_stream::McpEventStreams;
pub(crate) use mcp_processor::McpRequestProcessor;
pub(crate) use plugins::PluginRequestProcessor;
pub(crate) use process_exec_processor::ProcessExecRequestProcessor;
pub(crate) use projects::ProjectRequestProcessor;
pub(crate) use remote_control_processor::RemoteControlRequestProcessor;
pub(crate) use search::SearchRequestProcessor;
pub(crate) use thread_goal_processor::ThreadGoalRequestProcessor;
pub(crate) use thread_processor::ThreadRequestProcessor;
pub(crate) use thread_processor::ThreadResumeTarget;
pub(crate) use thread_queue_processor::ThreadQueueRequestProcessor;
pub(crate) use turn_processor::TurnRequestProcessor;
pub(crate) use windows_sandbox_processor::WindowsSandboxRequestProcessor;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::filters::compute_source_filters;
use crate::filters::source_kind_matches;
use crate::thread_state::ConnectionCapabilities;
use crate::thread_state::PendingThreadUnloads;
use crate::thread_state::ThreadListenerCommand;
use crate::thread_state::ThreadState;
use crate::thread_state::ThreadStateManager;
use token_usage_replay::restored_token_usage_turn_id;
use token_usage_replay::send_thread_token_usage_update_to_connection;

pub(crate) fn apply_live_thread_settings(
    thread: &mut Thread,
    config_snapshot: &ThreadConfigSnapshot,
) {
    thread.model = Some(config_snapshot.model.clone());
    thread.reasoning_effort = config_snapshot.reasoning_effort.clone();
    thread.environments = Some(
        config_snapshot
            .environment_selections()
            .iter()
            .map(Into::into)
            .collect(),
    );
}

fn resolve_request_cwd(cwd: Option<PathBuf>) -> Result<Option<AbsolutePathBuf>, JSONRPCErrorError> {
    cwd.map(|cwd| {
        AbsolutePathBuf::relative_to_current_dir(path_utils::normalize_for_native_workdir(cwd))
            .map_err(|err| invalid_request(format!("invalid cwd: {err}")))
    })
    .transpose()
}

fn resolve_turn_environment_requests(
    thread_manager: &ThreadManager,
    environments: Option<Vec<TurnEnvironmentParams>>,
) -> Result<Option<Vec<TurnEnvironmentRequest>>, JSONRPCErrorError> {
    let Some(environments) = environments else {
        return Ok(None);
    };
    let mut requests = Vec::with_capacity(environments.len());
    for environment in environments {
        let environment_id = environment.environment_id;
        let cwd = environment
            .cwd
            .to_inferred_path_uri()
            .ok_or_else(|| {
                invalid_request(format!(
                    "invalid cwd for environment `{environment_id}`: path `{}` does not use absolute POSIX or Windows path syntax",
                    environment.cwd
                ))
            })?;
        let workspace_roots = environment
            .runtime_workspace_roots
            .map(|roots| {
                let mut resolved_roots = Vec::new();
                for root in roots {
                    let root = root.to_inferred_path_uri().ok_or_else(|| {
                        invalid_request(format!(
                            "invalid runtime workspace root for environment `{environment_id}`: path `{root}` does not use absolute POSIX or Windows path syntax"
                        ))
                    })?;
                    if !resolved_roots.contains(&root) {
                        resolved_roots.push(root);
                    }
                }
                Ok::<_, JSONRPCErrorError>(resolved_roots)
            })
            .transpose()?
            .unwrap_or_else(|| vec![cwd.clone()]);
        requests.push(TurnEnvironmentRequest {
            environment_id,
            cwd,
            workspace_roots,
            config: EnvironmentConfigState::FromThread,
        });
    }
    validate_environment_ids_and_cwds(
        &thread_manager.environment_manager(),
        requests
            .iter()
            .map(|request| (request.environment_id.as_str(), &request.cwd)),
    )
    .map_err(environment_selection_error)?;
    Ok(Some(requests))
}

fn resolve_runtime_workspace_roots(workspace_roots: Vec<AbsolutePathBuf>) -> Vec<AbsolutePathBuf> {
    let mut resolved_roots = Vec::new();
    for root in workspace_roots {
        if !resolved_roots.iter().any(|existing| existing == &root) {
            resolved_roots.push(root);
        }
    }
    resolved_roots
}

mod config_errors;
mod request_errors;
mod thread_delete;
mod thread_goal_processor;
mod thread_lifecycle;
mod thread_resume_redaction;
mod thread_summary;

use self::config_errors::*;
use self::request_errors::*;
use self::thread_goal_processor::api_thread_goal_from_state;
use self::thread_lifecycle::*;
use self::thread_resume_redaction::*;
use self::thread_summary::*;

pub(crate) use self::thread_lifecycle::populate_thread_turns_from_history;
pub(crate) use self::thread_processor::thread_from_stored_thread;
#[cfg(test)]
pub(crate) use self::thread_summary::read_summary_from_rollout;
#[cfg(test)]
pub(crate) use self::thread_summary::summary_to_thread;
pub(crate) use self::thread_summary::thread_settings_from_config_snapshot;

pub(crate) fn build_legacy_api_turns_from_rollout_items(items: &[RolloutItem]) -> Vec<Turn> {
    let mut builder = ThreadHistoryBuilder::new();
    for item in items {
        if let Some(item) =
            persisted_rollout_item(item, nez_protocol::protocol::ThreadHistoryMode::Legacy)
        {
            builder.handle_rollout_item(item.as_ref());
        }
    }
    builder.finish()
}
