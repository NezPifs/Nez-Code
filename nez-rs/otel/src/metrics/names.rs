use std::sync::LazyLock;

pub const TOOL_CALL_COUNT_METRIC: &str = "nez.tool.call";
pub const TOOL_CALL_DURATION_METRIC: &str = "nez.tool.call.duration_ms";
pub const TOOL_CALL_UNIFIED_EXEC_METRIC: &str = "nez.tool.unified_exec";
pub const MULTI_AGENT_SPAWN_FAILURE_METRIC: &str = "nez.multi_agent.spawn.failure";
pub const MULTI_AGENT_SPAWN_PHASE_DURATION_METRIC: &str = "nez.multi_agent.spawn.phase.duration_ms";
pub const ARTIFACT_OPERATION_STARTED_METRIC: &str = "nez.artifact.operation.started";
pub const ARTIFACT_OPERATION_EXPECTED_OUTPUT_COUNT_METRIC: &str =
    "nez.artifact.operation.expected_output_count";
pub const PROCESS_START_METRIC: &str = "nez.process.start";
/// Caller-side exec-server RPC attempts, including local admission and transport failures.
pub const EXEC_SERVER_CLIENT_REQUEST_COUNT_METRIC: &str = "exec_server_client_requests_total";
pub const API_CALL_COUNT_METRIC: &str = "nez.api_request";
pub const API_CALL_DURATION_METRIC: &str = "nez.api_request.duration_ms";
pub const SSE_EVENT_COUNT_METRIC: &str = "nez.sse_event";
pub const SSE_EVENT_DURATION_METRIC: &str = "nez.sse_event.duration_ms";
pub const WEBSOCKET_REQUEST_COUNT_METRIC: &str = "nez.websocket.request";
pub const WEBSOCKET_CONTINUATION_COUNT_METRIC: &str = "nez.websocket.continuation";
pub const WEBSOCKET_REQUEST_DURATION_METRIC: &str = "nez.websocket.request.duration_ms";
pub const WEBSOCKET_EVENT_COUNT_METRIC: &str = "nez.websocket.event";
pub const WEBSOCKET_EVENT_DURATION_METRIC: &str = "nez.websocket.event.duration_ms";
pub const RESPONSES_API_OVERHEAD_DURATION_METRIC: &str = "nez.responses_api_overhead.duration_ms";
pub const RESPONSES_API_INFERENCE_TIME_DURATION_METRIC: &str =
    "nez.responses_api_inference_time.duration_ms";
pub const RESPONSES_API_ENGINE_IAPI_TTFT_DURATION_METRIC: &str =
    "nez.responses_api_engine_iapi_ttft.duration_ms";
pub const RESPONSES_API_ENGINE_SERVICE_TTFT_DURATION_METRIC: &str =
    "nez.responses_api_engine_service_ttft.duration_ms";
pub const RESPONSES_API_ENGINE_IAPI_TBT_DURATION_METRIC: &str =
    "nez.responses_api_engine_iapi_tbt.duration_ms";
pub const RESPONSES_API_ENGINE_SERVICE_TBT_DURATION_METRIC: &str =
    "nez.responses_api_engine_service_tbt.duration_ms";
pub const TURN_E2E_DURATION_METRIC: &str = "nez.turn.e2e_duration_ms";
pub const TURN_TTFT_DURATION_METRIC: &str = "nez.turn.ttft.duration_ms";
pub const TURN_TTFM_DURATION_METRIC: &str = "nez.turn.ttfm.duration_ms";
pub const TURN_NETWORK_PROXY_METRIC: &str = "nez.turn.network_proxy";
pub const TURN_MEMORY_METRIC: &str = "nez.turn.memory";
pub const TURN_TOOL_CALL_METRIC: &str = "nez.turn.tool.call";
pub const TURN_TOKEN_USAGE_METRIC: &str = "nez.turn.token_usage";
pub const TURN_COST_MICROUSD_METRIC: &str = "nez.turn.cost_microusd";
pub const TURN_UNIFIED_EXEC_RUNNING_PROCESSES_METRIC: &str =
    "nez.turn.unified_exec.running_processes";
pub const GUARDIAN_REVIEW_COUNT_METRIC: &str = "nez.guardian.review";
pub const GUARDIAN_REVIEW_DURATION_METRIC: &str = "nez.guardian.review.duration_ms";
pub const GUARDIAN_REVIEW_TTFT_DURATION_METRIC: &str = "nez.guardian.review.ttft.duration_ms";
pub const GUARDIAN_REVIEW_TOKEN_USAGE_METRIC: &str = "nez.guardian.review.token_usage";
pub const GUARDIAN_REVIEW_TRUNCATION_METRIC: &str = "nez.guardian.review.truncation";
pub const GUARDIAN_REVIEW_TRUNCATION_BYTES_METRIC: &str = "nez.guardian.review.truncation.bytes";
pub const GOAL_CREATED_METRIC: &str = "nez.goal.created";
pub const GOAL_RESUMED_METRIC: &str = "nez.goal.resumed";
pub const GOAL_COMPLETED_METRIC: &str = "nez.goal.completed";
pub const GOAL_BUDGET_LIMITED_METRIC: &str = "nez.goal.budget_limited";
pub const GOAL_USAGE_LIMITED_METRIC: &str = "nez.goal.usage_limited";
pub const GOAL_BLOCKED_METRIC: &str = "nez.goal.blocked";
pub const GOAL_TOKEN_COUNT_METRIC: &str = "nez.goal.token_count";
pub const GOAL_DURATION_SECONDS_METRIC: &str = "nez.goal.duration_s";
pub const PLUGIN_INSTALL_ELICITATION_SENT_METRIC: &str = "nez.plugins.install_elicitation.sent";
pub const PLUGIN_INSTALL_SUGGESTION_METRIC: &str = "nez.plugins.install_suggestion";
pub const CURATED_PLUGINS_STARTUP_SYNC_METRIC: &str = "nez.plugins.startup_sync";
pub const CURATED_PLUGINS_STARTUP_SYNC_FINAL_METRIC: &str = "nez.plugins.startup_sync.final";
pub const HOOK_RUN_METRIC: &str = "nez.hooks.run";
pub const HOOK_RUN_DURATION_METRIC: &str = "nez.hooks.run.duration_ms";
/// Duration for coarse startup phases, tagged by low-cardinality phase and status.
pub const STARTUP_PHASE_DURATION_METRIC: &str = "nez.startup.phase.duration_ms";
/// Total runtime of a startup prewarm attempt until it completes, tagged by final status.
pub const STARTUP_PREWARM_DURATION_METRIC: &str = "nez.startup_prewarm.duration_ms";
/// Age of the startup prewarm attempt when the first real turn resolves it, tagged by outcome.
pub const STARTUP_PREWARM_AGE_AT_FIRST_TURN_METRIC: &str =
    "nez.startup_prewarm.age_at_first_turn_ms";
pub const THREAD_STARTED_METRIC: &str = "nez.thread.started";
pub const THREAD_SKILLS_ENABLED_TOTAL_METRIC: &str = "nez.thread.skills.enabled_total";
pub const THREAD_SKILLS_KEPT_TOTAL_METRIC: &str = "nez.thread.skills.kept_total";
pub const THREAD_SKILLS_DESCRIPTION_TRUNCATED_CHARS_METRIC: &str =
    "nez.thread.skills.description_truncated_chars";
pub const THREAD_SKILLS_TRUNCATED_METRIC: &str = "nez.thread.skills.truncated";
// Tools measure the rendered block; kind=snapshot|delta distinguishes full catalogs from updates.
pub const THREAD_TOOLS_NAMESPACES_TOTAL_METRIC: &str = "nez.thread.tools.namespaces_total";
pub const THREAD_TOOLS_FRAGMENT_BYTES_METRIC: &str = "nez.thread.tools.fragment_bytes";

/// Finalized tool registrations per sampling request, tagged by exposure and tool_mode.
pub const TOOL_REGISTRATIONS_METRIC: &str = "nez.tools.registered";
pub const TOOL_REGISTRATIONS_METRIC_BUCKETS: &[f64] = &[
    0.0, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 50.0, 75.0, 100.0, 150.0, 200.0, 300.0, 500.0, 750.0,
    1_000.0, 2_000.0, 5_000.0, 10_000.0, 20_000.0,
];

/// Logarithmic boundaries up to 32,768 for tools bytes and namespace counts.
pub static THREAD_TOOLS_METRIC_BUCKETS: LazyLock<[f64; 511]> =
    LazyLock::new(|| context_log_buckets(/*max_exponent*/ 15.0));

/// Rendered incremental tool deltas, tagged by action; excludes initial catalogs.
/// schema_changed includes any definition change, such as description edits.
pub const TOOL_INCREMENTAL_UPDATES_METRIC: &str = "nez.tools.incremental_updates";

pub static THREAD_SKILLS_COUNT_METRIC_BUCKETS: LazyLock<[f64; 513]> =
    LazyLock::new(|| std::array::from_fn(|index| index as f64));

/// Logarithmic boundaries up to 131,072 removed description characters.
pub static THREAD_SKILLS_DESCRIPTION_TRUNCATED_CHARS_BUCKETS: LazyLock<[f64; 511]> =
    LazyLock::new(|| context_log_buckets(/*max_exponent*/ 17.0));

pub const THREAD_SKILLS_TRUNCATED_BUCKETS: &[f64] = &[0.0, 1.0];

fn context_log_buckets(max_exponent: f64) -> [f64; 511] {
    std::array::from_fn(|index| {
        if index == 0 {
            0.0
        } else {
            2.0_f64.powf(max_exponent * (index - 1) as f64 / 509.0)
        }
    })
}
