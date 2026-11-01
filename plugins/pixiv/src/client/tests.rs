use super::*;
use crate::{config, journal, validation};
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{atomic::AtomicBool, Mutex},
    time::Instant,
};
use upload_plugin_sdk::{Asset, AssetKind, PluginWorkspace, UploadTarget};

struct Server {
    base: String,
    worker: Option<thread::JoinHandle<()>>,
    requests: Arc<Mutex<Vec<String>>>,
}
impl Server {
    fn new(responses: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let output = requests.clone();
        let worker = thread::spawn(move || {
            for (status, body) in responses {
                let start = Instant::now();
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && start.elapsed() < Duration::from_secs(15) =>
                        {
                            thread::sleep(Duration::from_millis(10))
                        }
                        Err(e) => panic!("mock accept failed: {e}"),
                    }
                };
                handle(stream, status, &body, &output);
            }
        });
        Self {
            base,
            worker: Some(worker),
            requests,
        }
    }
    fn finish(mut self) -> Vec<String> {
        self.worker.take().unwrap().join().unwrap();
        self.requests.lock().unwrap().clone()
    }
}
fn handle(mut stream: TcpStream, status: u16, response: &str, requests: &Mutex<Vec<String>>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        assert!(!line.is_empty());
        headers.push_str(&line);
    }
    let size = headers
        .lines()
        .find_map(|l| {
            l.to_lowercase()
                .strip_prefix("content-length:")
                .and_then(|v| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    let mut body = vec![0; size];
    reader.read_exact(&mut body).unwrap();
    requests
        .lock()
        .unwrap()
        .push(format!("{headers}\r\n{}", String::from_utf8_lossy(&body)));
    write!(stream,"HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
}
fn page(user: &str) -> String {
    let state = json!({"api":{"token":"test-csrf"}}).to_string();
    let value = json!({"props":{"pageProps":{"isLoggedIn":true,"gaUserData":{"userId":user},"serverSerializedPreloadedState":state}}});
    format!("<script id=\"__NEXT_DATA__\" type=\"application/json\">{value}</script>")
}
fn ok(value: Value) -> (u16, String) {
    (200, json!({"error":false,"body":value}).to_string())
}
fn config() -> Config {
    Config {
        user_id: "123".into(),
        tags: vec!["test".into(), "art".into()],
        ai_type: "notAiGenerated".into(),
        visibility: "private".into(),
        rating: None,
        original: true,
        allow_tag_edit: false,
        allow_comment: true,
    }
}
fn publication(directory: &Path) -> Publication {
    let path = directory.join("picture.png");
    image::RgbaImage::new(2, 2).save(&path).unwrap();
    Publication {
        title: "test title".into(),
        body: "test caption".into(),
        assets: vec![Asset {
            id: "file1".into(),
            path: path.to_string_lossy().into(),
            name: "picture.png".into(),
            kind: AssetKind::Image,
            size: std::fs::metadata(path).unwrap().len(),
            modified_ms: 0,
            sha256: Some("test-hash".into()),
        }],
    }
}
use std::path::Path;
fn invoke(
    client: &PixivClient,
    publication: &Publication,
    directory: &Path,
    operation: &str,
) -> Result<upload_plugin_sdk::Receipt> {
    let cancelled = AtomicBool::new(false);
    let target = UploadTarget {
        id: "pixiv".into(),
        driver_id: "pixiv".into(),
        name: "test".into(),
        created_at: 0,
    };
    let workspace = PluginWorkspace {
        id: "default".into(),
        name: "test".into(),
        content_rating: serde_json::from_value(json!("r18")).unwrap(),
        data_dir: directory.to_string_lossy().into(),
    };
    let context = UploadContext {
        job_id: "attempt",
        operation_id: operation,
        archive_root: directory,
        cancelled: &cancelled,
        progress: &|_| Ok(()),
        target: Some(&target),
        workspace: Some(&workspace),
    };
    journal::upload(client, publication, &context, &config(), "r18", directory)
}

#[test]
fn multipart_posts_real_bytes_and_returns_confirmed_receipt_without_duplicate_retry() {
    let directory = tempfile::tempdir().unwrap();
    let mut publication = publication(directory.path());
    let mut second = publication.assets[0].clone();
    second.id = "file2".into();
    second.name = "second.png".into();
    publication.assets.push(second);
    let server = Server::new(vec![
        (200, page("123")),
        ok(json!({"showCaptcha":false,"illustUploadToken":"test-upload"})),
        ok(json!({"convertKey":"convert-123"})),
        ok(json!({"status":"COMPLETE","illustId":"987"})),
    ]);
    let client = PixivClient::connect_at("fake_session", "123", &server.base).unwrap();
    let receipt = invoke(&client, &publication, directory.path(), "operation1").unwrap();
    assert_eq!(receipt.location, "https://www.pixiv.net/artworks/987");
    assert_eq!(
        invoke(&client, &publication, directory.path(), "operation1")
            .unwrap()
            .location,
        receipt.location
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    let post = &requests[2];
    for field in [
        "POST /ajax/work/create/illustration ",
        "name=\"files[]\"; filename=\"picture.png\"",
        "name=\"imageOrder[0][fileKey]\"",
        "name=\"xRestrict\"\r\n\r\nr18",
        "name=\"aiType\"\r\n\r\nnotAiGenerated",
        "name=\"restrict\"\r\n\r\nprivate",
        "name=\"tags[]\"",
        "name=\"illustUploadToken\"",
        "x-csrf-token: test-csrf",
        "cookie: PHPSESSID=fake_session",
    ] {
        assert!(post.contains(field), "missing {field}");
    }
    assert!(requests[3]
        .starts_with("GET /ajax/work/create/illustration/progress?convertKey=convert-123 "));
    assert!(post.contains("name=\"imageOrder[1][fileKey]\"\r\n\r\n1"));
    assert!(post.contains("filename=\"second.png\""));
    assert!(
        !std::fs::read_to_string(directory.path().join("submissions/operation1.json"))
            .unwrap()
            .contains("fake_session")
    );
}
#[test]
fn incomplete_conversion_retries_query_without_posting_again() {
    let directory = tempfile::tempdir().unwrap();
    let publication = publication(directory.path());
    let server = Server::new(vec![
        (200, page("123")),
        ok(json!({})),
        ok(json!({"convertKey":"key"})),
        (503, "unavailable".into()),
        ok(json!({"status":"COMPLETE","illustId":321})),
    ]);
    let client = PixivClient::connect_at("fake", "123", &server.base).unwrap();
    assert_eq!(
        invoke(&client, &publication, directory.path(), "operation1")
            .unwrap_err()
            .code,
        "pixiv_http"
    );
    assert_eq!(
        invoke(&client, &publication, directory.path(), "operation1")
            .unwrap()
            .location,
        "https://www.pixiv.net/artworks/321"
    );
    let requests = server.finish();
    assert_eq!(
        requests.iter().filter(|r| r.starts_with("POST ")).count(),
        1
    );
}
#[test]
fn ambiguous_post_blocks_retry_and_new_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let publication = publication(directory.path());
    let server = Server::new(vec![
        (200, page("123")),
        ok(json!({})),
        (502, "error".into()),
    ]);
    let client = PixivClient::connect_at("fake", "123", &server.base).unwrap();
    for operation in ["op1", "op1", "op2"] {
        assert_eq!(
            invoke(&client, &publication, directory.path(), operation)
                .unwrap_err()
                .code,
            "pixiv_uncertain"
        );
    }
    assert_eq!(server.finish().len(), 3);
}
#[test]
fn captcha_stops_before_post_and_leaves_no_uncertain_submission() {
    let directory = tempfile::tempdir().unwrap();
    let publication = publication(directory.path());
    let server = Server::new(vec![(200, page("123")), ok(json!({"showCaptcha":true}))]);
    let client = PixivClient::connect_at("fake", "123", &server.base).unwrap();
    assert_eq!(
        invoke(&client, &publication, directory.path(), "op1")
            .unwrap_err()
            .code,
        "pixiv_captcha"
    );
    assert!(!directory.path().join("submissions/op1.json").exists());
    server.finish();
}
#[test]
fn account_mismatch_and_logged_out_html_are_rejected() {
    let server = Server::new(vec![(200, page("456"))]);
    assert_eq!(
        PixivClient::connect_at("fake", "123", &server.base)
            .err()
            .unwrap()
            .code,
        "pixiv_account"
    );
    server.finish();
    assert!(
        parse_login(&page("123").replace("\"isLoggedIn\":true", "\"isLoggedIn\":false")).is_err()
    );
    assert!(parse_login("<html>challenge</html>").is_err());
    assert!(config::validate_session("cookie; another=value").is_err());
}
#[test]
fn rate_limit_is_reported_without_retrying() {
    let server = Server::new(vec![(200, page("123")), (429, "limited".into())]);
    let client = PixivClient::connect_at("fake", "123", &server.base).unwrap();
    assert_eq!(client.init().unwrap_err().code, "pixiv_rate_limit");
    server.finish();
}
#[test]
fn format_size_and_animation_validation() {
    let directory = tempfile::tempdir().unwrap();
    let mut publication = publication(directory.path());
    assert!(validation::validate(&publication).is_empty());
    publication.title = "a".repeat(33);
    assert!(validation::validate(&publication)
        .iter()
        .any(|i| i.contains("标题")));
    publication.title = "ok".into();
    publication.assets[0].size = validation::FILE_LIMIT + 1;
    assert!(validation::validate(&publication)
        .iter()
        .any(|i| i.contains("32 MiB")));
    publication.assets[0].size = 10;
    let gif = directory.path().join("animated.gif");
    let mut encoder = image::codecs::gif::GifEncoder::new(File::create(&gif).unwrap());
    for _ in 0..2 {
        encoder
            .encode_frame(image::Frame::new(image::RgbaImage::new(2, 2)))
            .unwrap();
    }
    drop(encoder);
    publication.assets[0].path = gif.to_string_lossy().into();
    assert!(!validation::validate(&publication).is_empty());
    std::fs::write(&gif, b"not an image").unwrap();
    assert!(!validation::validate(&publication).is_empty());
}
#[test]
fn explicit_ai_rating_and_workspace_account_isolation() {
    let mut cfg = config();
    assert!(cfg.validate().is_ok());
    cfg.ai_type = "undefined".into();
    assert!(cfg.validate().is_err());
    cfg.ai_type = "aiGenerated".into();
    let directory = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let target = UploadTarget {
        id: "account-2".into(),
        driver_id: "pixiv".into(),
        name: "test".into(),
        created_at: 0,
    };
    let workspace = PluginWorkspace {
        id: "r18-space".into(),
        name: "test".into(),
        content_rating: serde_json::from_value(json!("r18")).unwrap(),
        data_dir: directory.path().to_string_lossy().into(),
    };
    let mut context = UploadContext {
        job_id: "j",
        operation_id: "j",
        archive_root: directory.path(),
        cancelled: &cancel,
        progress: &|_| Ok(()),
        target: Some(&target),
        workspace: Some(&workspace),
    };
    assert_eq!(
        config::account_dir(&context).unwrap(),
        directory.path().join("account-2")
    );
    assert_eq!(cfg.rating(&context).unwrap(), "r18");
    cfg.rating = Some("general".into());
    assert!(cfg.rating(&context).is_err());
    cfg.rating = None;
    let unspecified = PluginWorkspace {
        content_rating: serde_json::from_value(json!("unspecified")).unwrap(),
        ..workspace.clone()
    };
    context.workspace = Some(&unspecified);
    assert!(cfg.rating(&context).is_err());
}
#[test]
fn failed_conversion_never_returns_success() {
    let directory = tempfile::tempdir().unwrap();
    let publication = publication(directory.path());
    let server = Server::new(vec![
        (200, page("123")),
        ok(json!({})),
        ok(json!({"convertKey":"key"})),
        ok(json!({"status":"FAILURE"})),
    ]);
    let client = PixivClient::connect_at("fake", "123", &server.base).unwrap();
    assert_eq!(
        invoke(&client, &publication, directory.path(), "op1")
            .unwrap_err()
            .code,
        "pixiv_conversion"
    );
    server.finish();
}

#[test]
fn explicit_rejection_preserves_reason_and_allows_manual_retry() {
    let directory = tempfile::tempdir().unwrap();
    let publication = publication(directory.path());
    let server = Server::new(vec![
        (200, page("123")), ok(json!({})),
        (200, json!({"error":true,"message":"Invalid request","body":{"errors":{"tags":["Invalid tag"]}}}).to_string()),
        ok(json!({})), ok(json!({"convertKey":"retry-key"})),
        ok(json!({"status":"COMPLETE","illustId":987})),
    ]);
    let client = PixivClient::connect_at("fake", "123", &server.base).unwrap();
    let error = invoke(&client, &publication, directory.path(), "op1").unwrap_err();
    assert_eq!(error.code, "pixiv_rejected");
    assert!(error.message.contains("Invalid tag"));
    assert!(error.message.contains("Invalid request"));
    assert!(
        std::fs::read_to_string(directory.path().join("submissions/op1.json"))
            .unwrap()
            .contains("rejected")
    );
    assert!(invoke(&client, &publication, directory.path(), "op1").is_ok());
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.starts_with("POST "))
            .count(),
        2
    );
}

#[test]
fn unknown_envelope_and_contradictory_rejection_remain_uncertain() {
    for response in [
        json!({"message":"unknown"}),
        json!({"error":true,"body":{"convertKey":"possibly-accepted"}}),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let publication = publication(directory.path());
        let server = Server::new(vec![
            (200, page("123")),
            ok(json!({})),
            (200, response.to_string()),
        ]);
        let client = PixivClient::connect_at("fake", "123", &server.base).unwrap();
        for _ in 0..2 {
            assert_eq!(
                invoke(&client, &publication, directory.path(), "op1")
                    .unwrap_err()
                    .code,
                "pixiv_uncertain"
            );
        }
        assert_eq!(
            server
                .finish()
                .iter()
                .filter(|r| r.starts_with("POST "))
                .count(),
            1
        );
    }
}
