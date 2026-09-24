//! Codex `config.toml` 的原地编辑：只改 LLMux 负责的字段，保留用户其余配置。
//!
//! 用 toml_edit 按 section 精确定位（而不是字符串查找），因此：
//! - `model` 的更新不会误打到 `review_model` 上（后者的值含 `model = "` 子串）
//! - 不会改到用户其它 `[model_providers.*]` 段落里的同名键
//! - 缺失的键落到文档中正确的位置，而不是盲追加到文件末尾（那会把键塞进末尾的 section）
//! - 用户手写的注释、格式与无关配置原样保留

use toml_edit::{value, DocumentMut};

/// 基于现有 TOML 文本合并 LLMux 负责的字段，返回新的 TOML 文本。
///
/// 解析失败（用户手写的 TOML 有语法错误）时返回 `Err`，由调用方报错给用户 ——
/// 绝不吞掉错误后拿一个空文档覆盖原文件。
pub fn patch_codex_toml(
    existing: &str,
    model: &str,
    review_model: &str,
    api_base_url: &str,
    wire_api: &str,
    context_window: Option<u64>,
    auto_compact_limit: Option<u64>,
) -> Result<String, toml_edit::TomlError> {
    let mut doc: DocumentMut = existing.parse()?;

    // 文档根（顶层）键
    doc["model_provider"] = value("llmux");
    doc["model"] = value(model);
    doc["review_model"] = value(review_model);
    if let Some(v) = context_window {
        doc["model_context_window"] = value(v as i64);
    }
    if let Some(v) = auto_compact_limit {
        doc["model_auto_compact_token_limit"] = value(v as i64);
    }

    // provider 段落：toml_edit 会按需创建 [model_providers.llmux]，
    // 已存在的其它 provider 段落不受影响
    doc["model_providers"]["llmux"]["name"] = value("llmux");
    doc["model_providers"]["llmux"]["base_url"] = value(api_base_url);
    doc["model_providers"]["llmux"]["wire_api"] = value(wire_api);
    doc["model_providers"]["llmux"]["requires_openai_auth"] = value(true);

    Ok(doc.to_string())
}
