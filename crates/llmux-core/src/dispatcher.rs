use crate::adapters::Account;
use crate::crypto::decrypt_api_key;
use crate::models::ModelAlias;
use serde_json;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelResolution {
    /// 请求路由的厂商 id（无显式绑定时按此厂商取账户）。
    pub vendor_id: String,
    pub target_model: String,
    /// 显式绑定账户集（跨厂商），非空时优先于 vendor_id 分组。
    pub account_ids: Vec<i64>,
    /// 绑定集内首选账户（`is_preferred` 标记）。
    pub preferred_account_id: Option<i64>,
    pub alias_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Sticky-session dispatch router
// ---------------------------------------------------------------------------

const PROBE_TRIGGER_COUNT: u32 = 5;
const INITIAL_PROBE_BACKOFF_SECS: u64 = 30;
const MAX_PROBE_BACKOFF_SECS: u64 = 600;

/// The sticky routing mode for a single dispatch key.
#[derive(Debug, Clone)]
enum StickyMode {
    /// Using the preferred account. All requests go to it.
    Primary,
    /// Using a fallback account. Periodically probes preferred.
    Fallback {
        /// The account ID we are currently stuck to as fallback (for cache warmth).
        sticky_fallback_id: i64,
        /// Consecutive successful requests on fallback accounts.
        consecutive_successes: u32,
        /// When we last attempted to probe the preferred account.
        last_probe: Instant,
        /// Current probe backoff in seconds (doubles on each failed probe).
        probe_backoff_secs: u64,
    },
}

/// 当前墙钟的 unix 毫秒。
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 把持久化的墙钟毫秒还原成用于退避计时的 Instant。
///
/// 用「当前时刻减去已经过的时长」还原，使 elapsed() 自然等于「距上次探测过去了
/// 多久」——进程停机时长会被自动计入。停机超过 probe_backoff_secs 时，重启后
/// 首个请求即触发探测。
///
/// .max(0) 防时钟回拨（last_probe_ms 落在未来）时负数转 u64 溢出。
fn instant_from_millis(last_probe_ms: i64) -> Instant {
    let age_ms = (now_millis() - last_probe_ms).max(0) as u64;
    Instant::now() - Duration::from_millis(age_ms)
}

/// 粘滞路由状态的持久化行，与 dispatch_state 表列一一对应。
///
/// mode 为 "primary" / "fallback"，必须匹配表的 CHECK 约束；
/// Primary 行不承载其余字段的语义（restore 时忽略）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchStateRow {
    pub dispatch_key: String,
    pub mode: String,
    pub sticky_fallback_id: i64,
    pub consecutive_successes: i64,
    pub last_probe_ms: i64,
    pub probe_backoff_secs: i64,
}

#[derive(Debug, Clone)]
struct StickyEntry {
    mode: StickyMode,
}

impl Default for StickyEntry {
    fn default() -> Self {
        Self {
            mode: StickyMode::Primary,
        }
    }
}

/// Metadata about a dispatch decision, returned by `DispatchRouter::select`.
#[derive(Debug, Clone)]
pub struct DispatchMeta {
    pub is_probe: bool,
    pub preferred_id: i64,
}

const MAX_ENTRIES: usize = 1024;

/// Sticky-session routing state machine with failover and exponential backoff.
///
/// 注意：spec 要求 `dispatch_state` 表持久化回退状态；当前实现保持内存态
/// （Instant 计时不可序列化），表已建好供后续持久化改造使用。
#[derive(Debug, Clone, Default)]
pub struct DispatchRouter {
    entries: HashMap<String, StickyEntry>,
    /// 是否存在尚未落盘的状态变更（由后台 flush 任务消费）。
    dirty: bool,
}

impl DispatchRouter {
    /// Determine the ordered list of accounts to try for this request.
    pub fn select(
        &mut self,
        dispatch_key: &str,
        accounts: &[Account],
        preferred_id: i64,
    ) -> (Vec<Account>, DispatchMeta) {
        let mut changed = false;
        let result = {
            let entry = self.entries.entry(dispatch_key.to_string()).or_default();

            let preferred_exists = accounts.iter().any(|a| a.id == preferred_id);
            if !preferred_exists && !matches!(entry.mode, StickyMode::Primary) {
                // 首选账户已不在该 alias 的绑定中 → 强制回到 Primary
                entry.mode = StickyMode::Primary;
                changed = true;
            }

            match &entry.mode {
                StickyMode::Primary => {
                    let ordered = order_with_preferred_first(accounts, preferred_id);
                    (
                        ordered,
                        DispatchMeta {
                            is_probe: false,
                            preferred_id,
                        },
                    )
                }
                StickyMode::Fallback {
                    sticky_fallback_id,
                    consecutive_successes,
                    last_probe,
                    probe_backoff_secs,
                } => {
                    let should_probe = *consecutive_successes >= PROBE_TRIGGER_COUNT
                        || last_probe.elapsed().as_secs() >= *probe_backoff_secs;

                    if should_probe {
                        let ordered = order_with_preferred_first(accounts, preferred_id);
                        (
                            ordered,
                            DispatchMeta {
                                is_probe: true,
                                preferred_id,
                            },
                        )
                    } else {
                        let ordered =
                            order_with_fallback_first(accounts, preferred_id, *sticky_fallback_id);
                        (
                            ordered,
                            DispatchMeta {
                                is_probe: false,
                                preferred_id,
                            },
                        )
                    }
                }
            }
        };
        if changed {
            self.dirty = true;
        }
        result
    }

    /// Update state after a dispatch attempt completes.
    /// `used_account_id` is `Some(id)` when a specific account handled the
    /// request, or `None` when no account could be reached.
    pub fn record_result(
        &mut self,
        dispatch_key: &str,
        meta: &DispatchMeta,
        used_account_id: Option<i64>,
        success: bool,
    ) {
        // 淘汰本身也是必须落盘的变更（删除内存中的 entry 需要同步删除表里的行）
        let mut changed = self.maybe_evict();
        {
            let entry = self.entries.entry(dispatch_key.to_string()).or_default();

            match &mut entry.mode {
                StickyMode::Primary => {
                    if !success || used_account_id != Some(meta.preferred_id) {
                        let sticky = used_account_id.filter(|&id| id != meta.preferred_id);
                        entry.mode = StickyMode::Fallback {
                            sticky_fallback_id: sticky.unwrap_or(0),
                            consecutive_successes: if sticky.is_some() { 1 } else { 0 },
                            last_probe: Instant::now(),
                            probe_backoff_secs: INITIAL_PROBE_BACKOFF_SECS,
                        };
                        changed = true;
                    }
                }
                StickyMode::Fallback {
                    sticky_fallback_id,
                    consecutive_successes,
                    last_probe,
                    probe_backoff_secs,
                } => {
                    if meta.is_probe {
                        if success && used_account_id == Some(meta.preferred_id) {
                            entry.mode = StickyMode::Primary;
                            changed = true;
                        } else {
                            *probe_backoff_secs =
                                (*probe_backoff_secs * 2).min(MAX_PROBE_BACKOFF_SECS);
                            *consecutive_successes = 0;
                            *last_probe = Instant::now();
                            if let Some(id) = used_account_id {
                                if id != meta.preferred_id {
                                    *sticky_fallback_id = id;
                                }
                            }
                            changed = true;
                        }
                    } else if success {
                        *consecutive_successes += 1;
                        if let Some(id) = used_account_id {
                            *sticky_fallback_id = id;
                        }
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.dirty = true;
        }
    }

    /// Drop Primary entries when the map grows beyond MAX_ENTRIES.
    /// Fallback entries are kept — they represent active failover state.
    ///
    /// 返回是否发生了淘汰：删除内存中的 entry 必须同步删除表里的行，
    /// 因此淘汰本身也要让调用方标脏。
    fn maybe_evict(&mut self) -> bool {
        if self.entries.len() <= MAX_ENTRIES {
            return false;
        }
        self.entries.retain(|_, e| {
            matches!(e.mode, StickyMode::Fallback { .. })
        });
        tracing::info!(
            "DispatchRouter evicted stale entries, {} fallback entries retained",
            self.entries.len()
        );
        true
    }

    /// 导出全部 entry 供持久化（含 Primary，便于全量重写时对账）。
    pub fn snapshot(&self) -> Vec<DispatchStateRow> {
        self.entries
            .iter()
            .map(|(key, entry)| match &entry.mode {
                StickyMode::Primary => DispatchStateRow {
                    dispatch_key: key.clone(),
                    mode: "primary".to_string(),
                    sticky_fallback_id: 0,
                    consecutive_successes: 0,
                    last_probe_ms: 0,
                    probe_backoff_secs: 0,
                },
                StickyMode::Fallback {
                    sticky_fallback_id,
                    consecutive_successes,
                    last_probe,
                    probe_backoff_secs,
                } => {
                    // last_probe 是单调时钟，落盘换算成墙钟毫秒
                    let age_ms = last_probe.elapsed().as_millis() as i64;
                    DispatchStateRow {
                        dispatch_key: key.clone(),
                        mode: "fallback".to_string(),
                        sticky_fallback_id: *sticky_fallback_id,
                        consecutive_successes: *consecutive_successes as i64,
                        last_probe_ms: now_millis() - age_ms,
                        probe_backoff_secs: *probe_backoff_secs as i64,
                    }
                }
            })
            .collect()
    }

    /// 从持久化行重建 router。未知 mode 按 Primary 处理（表有 CHECK 约束，
    /// 此处仅作防御，避免脏数据导致 panic）。
    pub fn restore(rows: Vec<DispatchStateRow>) -> Self {
        let mut entries = HashMap::with_capacity(rows.len());
        for row in rows {
            let mode = if row.mode == "fallback" {
                StickyMode::Fallback {
                    sticky_fallback_id: row.sticky_fallback_id,
                    consecutive_successes: row.consecutive_successes.max(0) as u32,
                    last_probe: instant_from_millis(row.last_probe_ms),
                    probe_backoff_secs: row.probe_backoff_secs.max(0) as u64,
                }
            } else {
                StickyMode::Primary
            };
            entries.insert(row.dispatch_key, StickyEntry { mode });
        }
        Self {
            entries,
            dirty: false,
        }
    }

    /// 是否有未落盘的状态变更。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 由后台任务在取过快照后调用。
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }
}

fn order_with_preferred_first(accounts: &[Account], preferred_id: i64) -> Vec<Account> {
    let mut result: Vec<Account> = Vec::with_capacity(accounts.len());
    let mut preferred: Option<Account> = None;
    for a in accounts {
        if a.id == preferred_id {
            preferred = Some(a.clone());
        } else {
            result.push(a.clone());
        }
    }
    if let Some(p) = preferred {
        result.insert(0, p);
    }
    result
}

fn order_with_fallback_first(
    accounts: &[Account],
    preferred_id: i64,
    sticky_fallback_id: i64,
) -> Vec<Account> {
    let mut result: Vec<Account> = Vec::with_capacity(accounts.len());
    let mut sticky: Option<Account> = None;
    let mut preferred: Option<Account> = None;
    for a in accounts {
        if a.id == sticky_fallback_id {
            sticky = Some(a.clone());
        } else if a.id == preferred_id {
            preferred = Some(a.clone());
        } else {
            result.push(a.clone());
        }
    }
    if let Some(s) = sticky {
        result.insert(0, s);
    }
    if let Some(p) = preferred {
        result.push(p);
    }
    result
}

/// Strip ANSI escape sequences, control characters, and Claude Code
/// `[1m]` long-context suffix from model names.
pub fn sanitize_model_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip the entire ANSI escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // skip '['
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphabetic() {
                        chars.next(); // skip the terminator
                        break;
                    }
                    chars.next();
                }
            }
        } else if !ch.is_control() || ch == '\n' || ch == '\r' || ch == '\t' {
            result.push(ch);
        }
    }
    // Strip Claude Code [1m] long-context suffix (e.g. "d4p[1m]" → "d4p")
    if let Some(stripped) = result.strip_suffix("[1m]") {
        result = stripped.to_string();
    }
    result.trim().to_string()
}

/// 前缀回退：`claude-` → anthropic，`gemini-` → gemini，其余 → openai。
pub fn resolve_model_by_prefix(model_name: &str) -> ModelResolution {
    let vendor_id = if model_name.starts_with("claude-") {
        "anthropic"
    } else if model_name.starts_with("gemini-") || model_name.starts_with("models/gemini-") {
        "gemini"
    } else {
        "openai"
    };
    ModelResolution {
        vendor_id: vendor_id.to_string(),
        target_model: model_name.to_string(),
        account_ids: Vec::new(),
        preferred_account_id: None,
        alias_name: None,
    }
}

/// 解析模型名 → 路由信息（spec §4.3）：
/// - alias 有绑定 → JOIN model_alias_accounts 取精确账户集（is_preferred 优先）
/// - alias 无绑定但绑了 vendor → 按该厂商路由
/// - 无 alias → 前缀回退
pub async fn resolve_model(pool: &SqlitePool, model_name: &str) -> anyhow::Result<ModelResolution> {
    let model_name = sanitize_model_name(model_name);
    let alias = sqlx::query_as::<_, ModelAlias>(
        "SELECT id, alias, target_model, vendor_id, created_at FROM model_aliases WHERE alias = ?",
    )
    .bind(&model_name)
    .fetch_optional(pool)
    .await?;

    if let Some(alias) = alias {
        let alias_id = alias.id.unwrap_or_default();
        let bindings = sqlx::query_as::<_, (i64, i64)>(
            "SELECT account_id, is_preferred FROM model_alias_accounts
             WHERE alias_id = ? ORDER BY position, id",
        )
        .bind(alias_id)
        .fetch_all(pool)
        .await?;

        let account_ids: Vec<i64> = bindings.iter().map(|(a, _)| *a).collect();
        let preferred_account_id = bindings
            .iter()
            .find(|(_, preferred)| *preferred == 1)
            .map(|(a, _)| *a);
        let alias_name = Some(alias.alias.clone());

        if !account_ids.is_empty() {
            return Ok(ModelResolution {
                vendor_id: alias.vendor_id.unwrap_or_default(),
                target_model: alias.target_model,
                account_ids,
                preferred_account_id,
                alias_name,
            });
        }

        if let Some(vendor_id) = alias.vendor_id {
            return Ok(ModelResolution {
                vendor_id,
                target_model: alias.target_model,
                account_ids: Vec::new(),
                preferred_account_id,
                alias_name,
            });
        }
    }
    Ok(resolve_model_by_prefix(&model_name))
}

/// 账户选择公共 SQL：JOIN vendors 解析 protocol 与有效 base_url。
/// - 有效 openai URL = 账户自定义 base_url（非空）或厂商 default_base_url
/// - 有效 anthropic URL = 账户自定义 anthropic_base_url（非空）或厂商 default_anthropic_url / default_base_url
const ACCOUNT_SELECT: &str = "SELECT
    a.id, a.name, a.vendor_id, a.api_key_enc, a.enabled, a.weight, a.openai_compatible,
    v.protocol, v.protocols, v.openai_responses,
    a.base_url AS custom_base_url_raw,
    a.anthropic_base_url AS custom_anthropic_base_url_raw,
    COALESCE(NULLIF(a.base_url, ''), NULLIF(v.coding_base_url, ''), v.default_base_url) AS base_url,
    COALESCE(NULLIF(a.anthropic_base_url, ''), NULLIF(v.coding_anthropic_url, ''), NULLIF(v.coding_base_url, ''), v.default_anthropic_url, v.default_base_url) AS anthropic_base_url
  FROM accounts a
  JOIN vendors v ON a.vendor_id = v.id";

fn row_to_account(row: &sqlx::sqlite::SqliteRow, encryption_secret: &str) -> Option<Account> {
    let encrypted_key: String = match row.try_get("api_key_enc") {
        Ok(k) => k,
        Err(_) => {
            tracing::warn!("account row missing api_key_enc");
            return None;
        }
    };
    match decrypt_api_key(&encrypted_key, encryption_secret) {
        Ok(api_key) => {
            let custom_base_url = row
                .try_get::<Option<String>, _>("custom_base_url_raw")
                .ok()
                .flatten()
                .is_some_and(|u| !u.is_empty());
            let custom_anthropic_base_url = row
                .try_get::<Option<String>, _>("custom_anthropic_base_url_raw")
                .ok()
                .flatten()
                .is_some_and(|u| !u.is_empty());
            let protocol: String = row.try_get("protocol").unwrap_or_default();
            // 厂商声明支持 anthropic（vendors.protocols JSON 含 "anthropic"）→ 可服务 /v1/messages
            let protocols: Vec<String> = row
                .try_get::<Option<String>, _>("protocols")
                .ok()
                .flatten()
                .map(|s| serde_json::from_str(&s).unwrap_or_default())
                .unwrap_or_default();
            let serves_anthropic = protocol == "anthropic"
                || protocols.iter().any(|p| p == "anthropic");
            Some(Account {
                id: row.try_get("id").unwrap_or_default(),
                name: row.try_get("name").unwrap_or_default(),
                vendor_id: row.try_get("vendor_id").unwrap_or_default(),
                protocol,
                api_key,
                base_url: row.try_get("base_url").ok(),
                anthropic_base_url: row.try_get("anthropic_base_url").ok(),
                custom_base_url,
                custom_anthropic_base_url,
                serves_anthropic,
                openai_compatible: row.try_get("openai_compatible").unwrap_or(0),
                openai_responses: row.try_get::<i64, _>("openai_responses").unwrap_or(1) == 1,
                enabled: row.try_get("enabled").unwrap_or(1),
                weight: row.try_get("weight").unwrap_or(1),
            })
        }
        Err(e) => {
            let name: String = row.try_get("name").unwrap_or_default();
            let id: i64 = row.try_get("id").unwrap_or_default();
            tracing::warn!(
                "failed to decrypt API key for account {} (id={}), check MASTER_KEY: {e}",
                name,
                id
            );
            None
        }
    }
}

/// Load enabled accounts by explicit IDs（跨厂商绑定集）。
pub async fn get_accounts_by_ids(
    pool: &SqlitePool,
    ids: &[i64],
    encryption_secret: &str,
) -> anyhow::Result<Vec<Account>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "{ACCOUNT_SELECT} WHERE a.id IN ({}) AND a.enabled = 1 ORDER BY a.weight DESC, a.id ASC",
        placeholders.join(",")
    );
    let mut query = sqlx::query(&sql);
    for id in ids {
        query = query.bind(id);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows
        .iter()
        .filter_map(|row| row_to_account(row, encryption_secret))
        .collect())
}

/// 加载指定厂商（或全部）的 enabled 账户，按 weight 降序。
pub async fn get_active_accounts(
    pool: &SqlitePool,
    vendor_id: Option<&str>,
    encryption_secret: &str,
) -> anyhow::Result<Vec<Account>> {
    let rows = if let Some(vendor) = vendor_id {
        sqlx::query(&format!(
            "{ACCOUNT_SELECT} WHERE a.vendor_id = ? AND a.enabled = 1 ORDER BY a.weight DESC, a.id ASC"
        ))
        .bind(vendor)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(&format!(
            "{ACCOUNT_SELECT} WHERE a.enabled = 1 ORDER BY a.weight DESC, a.id ASC"
        ))
        .fetch_all(pool)
        .await?
    };
    Ok(rows
        .iter()
        .filter_map(|row| row_to_account(row, encryption_secret))
        .collect())
}

pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 429)
}
