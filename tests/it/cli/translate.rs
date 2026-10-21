use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{
    fails, library, library_with, ok, stderr, stdout, write_config_with, xuewen_with_stdin,
};

async fn llm(status: u16) -> MockServer {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(status).set_body_json(json!({
            "choices": [{"message": {"content": "你好"}}]
        })))
        .mount(&upstream)
        .await;
    upstream
}

async fn config_for(upstream: &MockServer) -> std::path::PathBuf {
    let (_pool, config) = library_with(&format!(
        "[ai]\nbase_url = {:?}\napi_key = \"k\"\n\n[ai.translate]\nmodel = \"mock-1\"\n",
        upstream.uri()
    ))
    .await;
    config
}

async fn sent_text(upstream: &MockServer) -> String {
    let requests = upstream.received_requests().await.unwrap();
    let body: serde_json::Value = requests.last().unwrap().body_json().unwrap();
    body["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn arguments_are_translated_as_one_text() {
    let upstream = llm(200).await;
    let config = config_for(&upstream).await;
    assert_eq!(
        ok(&config, &["translate", "hello", "world", "--to", "zh"]),
        "你好\n"
    );
    assert_eq!(sent_text(&upstream).await, "hello world");
}

#[tokio::test]
async fn stdin_is_read_without_text_or_with_a_dash() {
    let upstream = llm(200).await;
    let config = config_for(&upstream).await;
    for args in [&["translate"][..], &["translate", "-"]] {
        let out = xuewen_with_stdin(&config, args, "  hello\nthere\n");
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert_eq!(stdout(&out), "你好\n");
        assert_eq!(sent_text(&upstream).await, "hello\nthere");
    }
}

#[tokio::test]
async fn blank_text_is_refused_before_any_call() {
    let upstream = llm(200).await;
    let config = config_for(&upstream).await;
    let out = xuewen_with_stdin(&config, &["translate"], " \n");
    assert!(!out.status.success());
    assert!(stderr(&out).contains("empty text"), "got: {}", stderr(&out));
    assert!(upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn provider_failures_and_unknown_names_are_reported() {
    let upstream = llm(500).await;
    let config = config_for(&upstream).await;
    for (args, expected) in [
        (&["translate", "hello"][..], "translation failed: "),
        (
            &["translate", "hello", "--provider", "deepl"],
            "translation failed: DeepL translate provider not configured",
        ),
        (
            &["translate", "hello", "--provider", "babel"],
            "unknown provider `babel`",
        ),
    ] {
        let err = fails(&config, args);
        assert!(err.contains(expected), "{args:?}: {err}");
    }
}

#[tokio::test]
async fn an_unconfigured_translate_says_what_to_set() {
    let (_pool, config) = library().await;
    let err = fails(&config, &["translate", "hello"]);
    assert!(
        err.contains("translate is not configured — set [ai.translate] (LLM) or [translate.deepl]"),
        "got: {err}"
    );

    let config = write_config_with(
        config.parent().unwrap(),
        "[ai]\napi_key = \"k\"\n\n[ai.translate]\n",
    );
    let err = fails(&config, &["translate", "hello"]);
    assert!(err.contains("[ai.translate] has no model"), "got: {err}");
}
