//! Codex `config.toml` 编辑的回归测试。
//!
//! 前三个场景都对应实机复现过的破坏性行为：误匹配子串、忽略 section 边界、
//! 把缺失键盲追加到文件末尾（导致键落进别的 section）。
//! 断言一律先解析回 TOML 再按键取值 —— 字符串匹配无法区分「键在顶层」和
//! 「键在某个 [section] 里」，而那正是这几个 bug 的本质。

use llmux_server::routes::system::codex_toml::patch_codex_toml;
use toml_edit::DocumentMut;

fn parse(s: &str) -> DocumentMut {
    s.parse().expect("生成的 TOML 必须可解析")
}

/// 缺省参数下的调用（测试里的输入都是合法 TOML，解析不应失败）。
fn patch(existing: &str, model: &str, review_model: &str) -> String {
    patch_codex_toml(existing, model, review_model, "http://llmux/v1", "chat", None, None)
        .expect("输入是合法 TOML")
}

#[test]
fn model_is_updated_even_when_review_model_comes_first() {
    // review_model 出现在 model 之前时，旧的字符串替换会把 model 的更新
    // 打到 review_model 行上（因为 "review_model = \"" 含子串 "model = \""）
    let existing = "review_model = \"old-review\"\nmodel = \"old-model\"\n";
    let out = patch(existing, "NEW-MODEL", "NEW-REVIEW");
    let doc = parse(&out);

    assert_eq!(doc["model"].as_str(), Some("NEW-MODEL"), "model 必须被更新");
    assert_eq!(doc["review_model"].as_str(), Some("NEW-REVIEW"));
}

#[test]
fn existing_other_provider_is_left_untouched() {
    // 旧的字符串替换会把 name/base_url/wire_api 改到文件里第一个匹配处，
    // 也就是用户原有的 provider 段落，同时让 llmux 段落缺这几个键
    let existing = "[model_providers.openai]\n\
                    name = \"openai\"\n\
                    base_url = \"https://api.openai.com/v1\"\n\
                    wire_api = \"responses\"\n";
    let out = patch(existing, "gpt-5", "gpt-5");
    let doc = parse(&out);

    let openai = &doc["model_providers"]["openai"];
    assert_eq!(openai["name"].as_str(), Some("openai"), "用户原有的 provider 不得被改动");
    assert_eq!(openai["base_url"].as_str(), Some("https://api.openai.com/v1"));
    assert_eq!(openai["wire_api"].as_str(), Some("responses"));

    let llmux = &doc["model_providers"]["llmux"];
    assert_eq!(llmux["name"].as_str(), Some("llmux"), "llmux provider 必须自带 name");
    assert_eq!(llmux["base_url"].as_str(), Some("http://llmux/v1"));
    assert_eq!(llmux["wire_api"].as_str(), Some("chat"));
    assert_eq!(llmux["requires_openai_auth"].as_bool(), Some(true));
}

#[test]
fn missing_keys_land_at_top_level_not_inside_trailing_section() {
    // 文件末尾是 [mcp_servers.*] 时，旧实现会把缺失的键追加进那个段落
    let existing = "model_provider = \"llmux\"\n\
                    model = \"gpt-5\"\n\
                    \n\
                    [model_providers.llmux]\n\
                    name = \"llmux\"\n\
                    \n\
                    [mcp_servers.filesystem]\n\
                    command = \"npx\"\n";
    let out = patch_codex_toml(
        existing, "gpt-5", "gpt-5", "http://llmux/v1", "chat", Some(100000), None,
    )
    .expect("输入是合法 TOML");
    let doc = parse(&out);

    assert_eq!(doc["model_context_window"].as_integer(), Some(100000), "缺的顶层键必须落在顶层");
    assert_eq!(doc["review_model"].as_str(), Some("gpt-5"), "review_model 也必须落在顶层");

    let mcp = &doc["mcp_servers"]["filesystem"];
    assert!(mcp.get("model_context_window").is_none(), "MCP 段不得被写入无关键");
    assert!(mcp.get("review_model").is_none(), "MCP 段不得被写入无关键");
    assert_eq!(mcp["command"].as_str(), Some("npx"), "MCP 原有内容必须保留");
}

#[test]
fn user_comments_and_unrelated_keys_survive() {
    // config.toml 是用户手写文件，注释与未知键必须原样保留
    let existing = "# 我的 Codex 配置\n\
                    model = \"old\"\n\
                    disable_response_storage = true\n\
                    \n\
                    [tui]\n\
                    status_line_use_colors = true\n";
    let out = patch(existing, "gpt-5", "gpt-5");

    assert!(out.contains("# 我的 Codex 配置"), "注释必须保留");
    let doc = parse(&out);
    assert_eq!(doc["disable_response_storage"].as_bool(), Some(true), "无关键必须保留");
    assert_eq!(doc["tui"]["status_line_use_colors"].as_bool(), Some(true), "其它 section 必须保留");
}

#[test]
fn malformed_toml_is_rejected_instead_of_overwritten() {
    // 用户手写的 config.toml 有语法错误时，必须报错而不是吞掉错误后覆盖
    let broken = "model = \"unclosed\n[oops\n";
    let result = patch_codex_toml(broken, "gpt-5", "gpt-5", "http://llmux/v1", "chat", None, None);
    assert!(result.is_err(), "语法错误的 config.toml 必须返回 Err，不得静默覆盖");
}

#[test]
fn empty_existing_config_produces_valid_toml() {
    // 首次使用（config.toml 不存在）时现有内容为空串
    let out = patch("", "gpt-5", "gpt-5");
    let doc = parse(&out);

    assert_eq!(doc["model_provider"].as_str(), Some("llmux"));
    assert_eq!(doc["model"].as_str(), Some("gpt-5"));
    assert_eq!(doc["model_providers"]["llmux"]["base_url"].as_str(), Some("http://llmux/v1"));
}
