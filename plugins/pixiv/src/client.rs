use crate::{
    config::{validate_session, Config},
    validation,
};
use reqwest::{
    blocking::{
        multipart::{Form, Part},
        Client, Response,
    },
    header::{HeaderMap, HeaderValue, COOKIE, ORIGIN, REFERER},
    redirect::Policy,
};
use serde_json::Value;
use std::{
    fs::File,
    io::Read,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use upload_plugin_sdk::{AppError, Publication, Result, UploadContext};

const ORIGIN_URL: &str = "https://www.pixiv.net";
pub struct PixivClient {
    client: Client,
    base: String,
}
pub enum Progress {
    Running,
    Complete(String),
    Failed,
}
fn network(_: reqwest::Error) -> AppError {
    AppError::new(
        "pixiv_network",
        "Pixiv 请求失败或超时；请检查网络和登录状态",
    )
}
fn protocol() -> AppError {
    AppError::new(
        "pixiv_protocol",
        "Pixiv 返回格式已变化，请更新插件或在网页检查作品状态",
    )
}

impl PixivClient {
    pub fn connect(session: &str, expected_user: &str) -> Result<Self> {
        Self::connect_at(session, expected_user, ORIGIN_URL)
    }
    fn connect_at(session: &str, expected_user: &str, base: &str) -> Result<Self> {
        validate_session(session)?;
        let mut headers = HeaderMap::new();
        let mut cookie =
            HeaderValue::from_str(&format!("PHPSESSID={session}")).map_err(|_| protocol())?;
        cookie.set_sensitive(true);
        headers.insert(COOKIE, cookie);
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://www.pixiv.net/illustration/create"),
        );
        headers.insert(ORIGIN, HeaderValue::from_static(ORIGIN_URL));
        let make = |headers| {
            Client::builder()
                .default_headers(headers)
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(20))
                .timeout(Duration::from_secs(300))
                .user_agent("UploadHub-Pixiv/0.1 (+desktop illustration uploader)")
                .build()
                .map_err(network)
        };
        let client = make(headers.clone())?;
        let response = client.get(format!("{base}/")).send().map_err(network)?;
        let text = read_response(response)?;
        let (token, user) = parse_login(&text)?;
        if user != expected_user {
            return Err(AppError::new(
                "pixiv_account",
                "登录用户与配置中的 Pixiv 用户 ID 不一致，已停止上传",
            ));
        }
        let mut csrf = HeaderValue::from_str(&token).map_err(|_| protocol())?;
        csrf.set_sensitive(true);
        headers.insert("x-csrf-token", csrf);
        headers.insert(
            "x-user-id",
            HeaderValue::from_str(&user).map_err(|_| protocol())?,
        );
        Ok(Self {
            client: make(headers)?,
            base: base.into(),
        })
    }
    pub fn init(&self) -> Result<Value> {
        let body = self.get("/ajax/work/create/illustration/init", &[])?;
        if body.get("showCaptcha").and_then(Value::as_bool) == Some(true) {
            return Err(AppError::new(
                "pixiv_captcha",
                "Pixiv 要求验证码，请在网页完成验证后重试；插件不会自动处理验证码",
            ));
        }
        if body
            .get("redirectUrl")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
        {
            return Err(AppError::new(
                "pixiv_action_required",
                "Pixiv 要求先在网页完成账号操作，请打开投稿页面检查",
            ));
        }
        Ok(body)
    }
    pub fn posting_scripts(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/illustration/create", self.base))
            .send()
            .map_err(network)?;
        let html = read_response(response)?;
        Ok(html
            .split("src=\"")
            .skip(1)
            .filter_map(|part| part.split('"').next())
            .filter(|url| url.starts_with("https://s.pximg.net/") && url.ends_with(".js"))
            .map(str::to_owned)
            .collect())
    }
    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        envelope(
            self.client
                .get(format!("{}{path}", self.base))
                .query(query)
                .send()
                .map_err(network)?,
        )
    }
    pub fn submit(
        &self,
        publication: &Publication,
        context: &UploadContext<'_>,
        config: &Config,
        rating: &str,
        init: &Value,
    ) -> Result<String> {
        let counter = Arc::new(AtomicU64::new(0));
        let mut form = Form::new()
            .text("title", publication.title.clone())
            .text("caption", publication.body.clone())
            .text("xRestrict", rating.to_string())
            .text("aiType", config.ai_type.clone())
            .text("restrict", config.visibility.clone())
            .text("original", config.original.to_string())
            .text("allowTagEdit", config.allow_tag_edit.to_string())
            .text("allowComment", config.allow_comment.to_string())
            .text("sexual", "false")
            .text("responseAutoAccept", "false")
            .text("allowCitationWork", "false");
        for tag in &config.tags {
            form = form.text("tags[]", tag.clone());
        }
        if let Some(token) = init.get("illustUploadToken").and_then(Value::as_str) {
            form = form.text("illustUploadToken", token.to_owned());
        }
        for (index, asset) in publication.assets.iter().enumerate() {
            let file = File::open(&asset.path)?;
            if file.metadata()?.len() != asset.size {
                return Err(AppError::new("source_changed", "上传图片大小已改变"));
            }
            let part = Part::reader_with_length(
                CountingReader {
                    file,
                    counter: counter.clone(),
                },
                asset.size,
            )
            .file_name(asset.name.clone())
            .mime_str(validation::mime(&asset.path)?)
            .map_err(network)?;
            form = form
                .part("files[]", part)
                .text(format!("imageOrder[{index}][type]"), "newFile")
                .text(format!("imageOrder[{index}][fileKey]"), index.to_string());
        }
        let request = self
            .client
            .post(format!("{}/ajax/work/create/illustration", self.base))
            .multipart(form);
        let body = thread::scope(|scope| -> Result<Value> {
            let worker = scope.spawn(move || envelope(request.send().map_err(network)?));
            let mut last = 0;
            while !worker.is_finished() {
                context.check_cancelled()?;
                let value = counter.load(Ordering::Relaxed);
                if value > last {
                    (context.progress)(value)?;
                    last = value;
                }
                thread::sleep(Duration::from_millis(120));
            }
            worker
                .join()
                .map_err(|_| AppError::new("pixiv_network", "Pixiv 上传线程异常终止"))?
        })?;
        let key = body
            .get("convertKey")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() < 512)
            .ok_or_else(protocol)?;
        Ok(key.to_owned())
    }
    pub fn progress(&self, key: &str) -> Result<Progress> {
        let value = self.get(
            "/ajax/work/create/illustration/progress",
            &[("convertKey", key)],
        )?;
        match value.get("status").and_then(Value::as_str) {
            Some("RUNNING") => Ok(Progress::Running),
            Some("FAILURE") => Ok(Progress::Failed),
            Some("COMPLETE") => {
                let id = value
                    .get("illustId")
                    .and_then(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .or_else(|| v.as_u64().map(|v| v.to_string()))
                    })
                    .ok_or_else(protocol)?;
                if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(protocol());
                }
                Ok(Progress::Complete(id))
            }
            _ => Err(protocol()),
        }
    }
}
struct CountingReader {
    file: File,
    counter: Arc<AtomicU64>,
}
impl Read for CountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.file.read(buffer)?;
        self.counter.fetch_add(count as u64, Ordering::Relaxed);
        Ok(count)
    }
}
fn read_response(response: Response) -> Result<String> {
    let status = response.status().as_u16();
    if status == 401 || status == 403 || (300..400).contains(&status) {
        return Err(AppError::new(
            "pixiv_login",
            "Pixiv 登录失效或需要网页验证，请重新登录后配置凭据",
        ));
    }
    if status == 429 {
        return Err(AppError::new(
            "pixiv_rate_limit",
            "Pixiv 请求过于频繁，请稍后手动重试，并增加补传间隔",
        ));
    }
    if !(200..300).contains(&status) {
        return Err(AppError::new(
            "pixiv_http",
            format!("Pixiv 返回 HTTP {status}，请检查网页状态"),
        ));
    }
    let mut body = String::new();
    response
        .take(4 * 1024 * 1024 + 1)
        .read_to_string(&mut body)
        .map_err(|_| protocol())?;
    if body.len() > 4 * 1024 * 1024 {
        return Err(protocol());
    }
    Ok(body)
}
fn envelope(response: Response) -> Result<Value> {
    let value: Value = serde_json::from_str(&read_response(response)?).map_err(|_| protocol())?;
    match value.get("error").and_then(Value::as_bool) {
        Some(false) => {}
        Some(true) if value["body"]["convertKey"].is_null() => {
            let mut messages = Vec::new();
            collect_messages(&value["message"], &mut messages);
            collect_messages(&value["body"]["errors"], &mut messages);
            let detail: String = messages
                .join("；")
                .chars()
                .filter(|c| !c.is_control())
                .take(800)
                .collect();
            return Err(AppError::new(
                "pixiv_rejected",
                if detail.is_empty() {
                    "Pixiv 明确拒绝了投稿，但没有提供具体原因；请在网页投稿页检查".to_owned()
                } else {
                    format!("Pixiv 拒绝投稿：{detail}")
                },
            ));
        }
        _ => return Err(protocol()),
    }
    value.get("body").cloned().ok_or_else(protocol)
}
// Only report designated error messages, never dump headers, cookies or response bodies.
fn collect_messages(value: &Value, output: &mut Vec<String>) {
    if output.len() >= 16 {
        return;
    }
    match value {
        Value::String(text) if !text.trim().is_empty() => {
            output.push(text.chars().take(400).collect())
        }
        Value::Array(items) => {
            for item in items {
                collect_messages(item, output);
            }
        }
        Value::Object(items) => {
            for item in items.values() {
                collect_messages(item, output);
            }
        }
        _ => {}
    }
}

fn parse_login(html: &str) -> Result<(String, String)> {
    let id = html.find("id=\"__NEXT_DATA__\"").ok_or_else(|| {
        AppError::new(
            "pixiv_login",
            "无法读取 Pixiv 登录状态，请检查凭据或更新插件",
        )
    })?;
    let start = html[id..].find('>').ok_or_else(protocol)? + id + 1;
    let end = html[start..].find("</script>").ok_or_else(protocol)? + start;
    let root: Value = serde_json::from_str(&html[start..end]).map_err(|_| protocol())?;
    let props = &root["props"]["pageProps"];
    if props["isLoggedIn"].as_bool() != Some(true) {
        return Err(AppError::new(
            "pixiv_login",
            "Pixiv PHPSESSID 已失效，请重新配置登录",
        ));
    }
    let state = match &props["serverSerializedPreloadedState"] {
        Value::String(s) => serde_json::from_str::<Value>(s).map_err(|_| protocol())?,
        v @ Value::Object(_) => v.clone(),
        _ => return Err(protocol()),
    };
    let token = state["api"]["token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(protocol)?
        .to_owned();
    let user = props["gaUserData"]["userId"]
        .as_str()
        .or_else(|| state["userData"]["self"]["id"].as_str())
        .ok_or_else(protocol)?
        .to_owned();
    Ok((token, user))
}

#[cfg(test)]
mod tests;
