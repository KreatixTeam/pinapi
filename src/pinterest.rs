//! Pinterest internal-API client, ported from py3-pinterest (MIT,
//! https://github.com/bstoilov/py3-pinterest). Drives the same
//! `/resource/<Name>Resource/` endpoints Pinterest's web frontend uses.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::cookie::{CookieStore, Jar};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{json, Map, Value};

use crate::config::Settings;

const AGENT: &str = "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/61.0.3163.100 Safari/537.36";
const HOME_PAGE: &str = "https://www.pinterest.com/";
const CREATE_USER_SESSION: &str = "https://www.pinterest.com/resource/UserSessionResource/create/";
const DELETE_USER_SESSION: &str = "https://www.pinterest.com/resource/UserSessionResource/delete/";
const USER_RESOURCE: &str = "https://www.pinterest.com/resource/UserResource/get/";
const BOARDS_RESOURCE: &str = "https://www.pinterest.com/resource/BoardsResource/get/";
const CREATE_BOARD_RESOURCE: &str = "https://www.pinterest.com/resource/BoardResource/create/";
const DELETE_BOARD_RESOURCE: &str = "https://www.pinterest.com/resource/BoardResource/delete/";
const FOLLOW_BOARD_RESOURCE: &str = "https://www.pinterest.com/resource/ApiResource/update/";
const UNFOLLOW_BOARD_RESOURCE: &str = "https://www.pinterest.com/resource/ApiResource/delete/";
const FOLLOW_USER_RESOURCE: &str = "https://www.pinterest.com/resource/UserFollowResource/create/";
const UNFOLLOW_USER_RESOURCE: &str = "https://www.pinterest.com/resource/UserFollowResource/delete/";
const USER_FOLLOWING_RESOURCE: &str = "https://www.pinterest.com/resource/UserFollowingResource/get/";
const USER_FOLLOWERS_RESOURCE: &str = "https://www.pinterest.com/resource/UserFollowersResource/get/";
const PIN_RESOURCE_CREATE: &str = "https://www.pinterest.com/resource/PinResource/create/";
const REPIN_RESOURCE_CREATE: &str = "https://www.pinterest.com/resource/RepinResource/create/";
const DELETE_PIN_RESOURCE: &str = "https://www.pinterest.com/resource/PinResource/delete/";
const VISUAL_LIVE_SEARCH_RESOURCE: &str =
    "https://www.pinterest.com/resource/VisualLiveSearchResource/get/";
const TYPE_AHEAD_RESOURCE: &str = "https://www.pinterest.com/resource/AdvancedTypeaheadResource/get/";
const BOARD_RECOMMEND_RESOURCE: &str =
    "https://www.pinterest.com/resource/BoardContentRecommendationResource/get/";
const PINNABLE_IMAGES_RESOURCE: &str =
    "https://www.pinterest.com/resource/FindPinImagesResource/get/";
const BOARD_FEED_RESOURCE: &str = "https://www.pinterest.com/resource/BoardFeedResource/get/";
const USER_HOME_FEED_RESOURCE: &str = "https://www.pinterest.com/resource/UserHomefeedResource/get/";
const USER_PIN_RESOURCE: &str = "https://www.pinterest.com/resource/UserPinsResource/get/";
const BASE_SEARCH_RESOURCE: &str = "https://www.pinterest.com/resource/BaseSearchResource/get/";
const CREATE_COMMENT_RESOURCE: &str =
    "https://www.pinterest.com/resource/AggregatedCommentResource/create/";
const GET_PIN_COMMENTS_RESOURCE: &str =
    "https://www.pinterest.com/resource/AggregatedCommentFeedResource/get/";
const DELETE_COMMENT: &str = "https://www.pinterest.com/resource/AggregatedCommentResource/delete/";
const PIN_RESOURCE_GET: &str = "https://www.pinterest.com/resource/PinResource/get/";
const CONVERSATION_RESOURCE: &str = "https://www.pinterest.com/resource/ConversationsResource/get/";
const CONVERSATION_RESOURCE_CREATE: &str =
    "https://www.pinterest.com/resource/ConversationsResource/create/";
const LOAD_CONVERSATION: &str =
    "https://www.pinterest.com/resource/ConversationMessagesResource/get/";
const SEND_MESSAGE: &str = "https://www.pinterest.com/resource/ConversationMessagesResource/create/";
const BOARD_SECTION_RESOURCE: &str = "https://www.pinterest.com/resource/BoardSectionResource/create/";
const GET_BOARD_SECTIONS: &str = "https://www.pinterest.com/resource/BoardSectionsResource/get/";
const BOARD_SECTION_EDIT_RESOURCE: &str =
    "https://www.pinterest.com/resource/BoardSectionEditResource/delete/";
const GET_BOARD_SECTION_PINS: &str =
    "https://www.pinterest.com/resource/BoardSectionPinsResource/get/";
const API_RESOURCE_CREATE: &str = "https://www.pinterest.com/resource/ApiResource/create/";
const STORY_PIN_RESOURCE_CREATE: &str =
    "https://www.pinterest.com/resource/StoryPinResource/create/";
const VIP_RESOURCE_GET: &str = "https://www.pinterest.com/resource/VIPResource/get/";
const S3_UPLOAD_URL: &str = "https://pinterest-media-upload.s3-accelerate.amazonaws.com/";

#[derive(Debug)]
pub struct PinError {
    pub status: u16,
    pub detail: String,
}

impl PinError {
    pub fn new(status: u16, detail: impl Into<String>) -> Self {
        Self { status, detail: detail.into() }
    }
}

impl fmt::Display for PinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for PinError {}

impl From<reqwest::Error> for PinError {
    fn from(e: reqwest::Error) -> Self {
        let status = e.status().map(|s| s.as_u16()).unwrap_or(502);
        PinError::new(status, e.to_string())
    }
}

impl From<serde_json::Error> for PinError {
    fn from(e: serde_json::Error) -> Self {
        PinError::new(502, e.to_string())
    }
}

type Result<T> = std::result::Result<T, PinError>;

pub struct Pinterest {
    http: reqwest::Client,
    jar: Arc<Jar>,
    base_url: reqwest::Url,
    bookmarks: Mutex<HashMap<String, String>>,
    settings: Settings,
    cookie_file: PathBuf,
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

fn encode(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// Serialize like Python's json.dumps default (", " / ": " separators) —
/// Pinterest's resource endpoints reject compact JSON as an invalid request.
fn py_json(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => serde_json::to_string(s).unwrap_or_default(),
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(py_json).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| {
                    format!("{}: {}", serde_json::to_string(k).unwrap_or_default(), py_json(v))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn data_payload(options: &Value) -> String {
    format!("{{\"options\": {}, \"context\": null}}", py_json(options))
}

impl Pinterest {
    pub fn new(settings: &Settings) -> Result<Self> {
        let jar = Arc::new(Jar::default());
        let base_url = reqwest::Url::parse(HOME_PAGE).unwrap();

        let cookie_file = Path::new(&settings.cred_root).join("cookies.json");
        if let Ok(text) = std::fs::read_to_string(&cookie_file) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&text) {
                for (k, v) in map {
                    jar.add_cookie_str(&format!("{k}={v}"), &base_url);
                }
            }
        }
        if let Some(raw) = &settings.pinterest_cookies {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(raw) {
                for (k, v) in map {
                    jar.add_cookie_str(&format!("{k}={v}"), &base_url);
                }
            }
        }

        let mut builder = reqwest::Client::builder()
            .cookie_provider(jar.clone())
            .user_agent(AGENT)
            .timeout(std::time::Duration::from_secs(60));
        if let Some(proxy) = &settings.proxy {
            builder = builder
                .proxy(reqwest::Proxy::all(proxy).map_err(|e| PinError::new(500, e.to_string()))?);
        }
        let http = builder.build().map_err(|e| PinError::new(500, e.to_string()))?;

        Ok(Self {
            http,
            jar,
            base_url,
            bookmarks: Mutex::new(HashMap::new()),
            settings: settings.clone(),
            cookie_file,
        })
    }

    // ---------- plumbing ----------

    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("Referer", HeaderValue::from_static(HOME_PAGE));
        h.insert("X-Requested-With", HeaderValue::from_static("XMLHttpRequest"));
        h.insert("Accept", HeaderValue::from_static("application/json"));
        h.insert(
            "Content-Type",
            HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"),
        );
        h.insert("User-Agent", HeaderValue::from_static(AGENT));
        if let Some(csrf) = self.cookie("csrftoken") {
            if let Ok(v) = HeaderValue::from_str(&csrf) {
                h.insert("X-CSRFToken", v);
            }
        }
        h
    }

    fn cookie(&self, name: &str) -> Option<String> {
        let header = self.jar.cookies(&self.base_url)?;
        let s = header.to_str().ok()?;
        for pair in s.split(';') {
            let pair = pair.trim();
            if let Some((k, v)) = pair.split_once('=') {
                if k.trim() == name {
                    return Some(v.trim().to_string());
                }
            }
        }
        None
    }

    fn persist_cookies(&self) {
        let Some(header) = self.jar.cookies(&self.base_url) else { return };
        let Ok(s) = header.to_str() else { return };
        let mut map = HashMap::new();
        for pair in s.split(';') {
            let pair = pair.trim();
            if let Some((k, v)) = pair.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        if let Some(dir) = self.cookie_file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&map) {
            let _ = std::fs::write(&self.cookie_file, text);
        }
    }

    fn build_data(options: Value) -> String {
        format!(
            "source_url={}&data={}&_={}",
            encode("/"),
            encode(&data_payload(&options)),
            now_ms()
        )
    }

    fn build_get(url: &str, options: Value, source_url: &str) -> String {
        format!(
            "{}?source_url={}&data={}&_={}",
            url,
            encode(source_url),
            encode(&data_payload(&options)),
            now_ms()
        )
    }

    fn build_post(options: Value, source_url: &str) -> String {
        format!(
            "source_url={}&data={}&_={}",
            encode(source_url),
            encode(&data_payload(&options)),
            now_ms()
        )
    }

    async fn get(&self, url: &str) -> Result<Value> {
        self.get_headers(url, HeaderMap::new()).await
    }

    async fn get_headers(&self, url: &str, extra: HeaderMap) -> Result<Value> {
        let mut h = self.headers();
        h.entry("X-Pinterest-PWS-Handler")
            .or_insert(HeaderValue::from_static("www/[username].js"));
        h.extend(extra);
        let resp = self.http.get(url).headers(h).send().await?;
        self.persist_cookies();
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(PinError::new(
                status.as_u16(),
                format!("{} for url: {}", status.canonical_reason().unwrap_or("error"), url)
                    + &format!(" — {}", &text[..text.len().min(300)]),
            ));
        }
        Ok(resp.json().await?)
    }

    async fn post(&self, url: &str, body: &str) -> Result<Value> {
        self.post_headers(url, body, HeaderMap::new()).await
    }

    async fn post_headers(&self, url: &str, body: &str, extra: HeaderMap) -> Result<Value> {
        let mut h = self.headers();
        h.extend(extra);
        let resp = self.http.post(url).headers(h).body(body.to_string()).send().await?;
        self.persist_cookies();
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(PinError::new(
                status.as_u16(),
                format!("{} for url: {}", status.canonical_reason().unwrap_or("error"), url)
                    + &format!(" — {}", &text[..text.len().min(300)]),
            ));
        }
        Ok(resp.json().await.unwrap_or(Value::Null))
    }

    // ---------- bookmarks ----------

    fn bm_key(primary: &str, secondary: Option<&str>) -> String {
        format!("{}::{}", primary, secondary.unwrap_or(""))
    }

    fn next_bookmark(&self, primary: &str, secondary: Option<&str>) -> Value {
        self.bookmarks
            .lock()
            .unwrap()
            .get(&Self::bm_key(primary, secondary))
            .map(|s| Value::String(s.clone()))
            .unwrap_or(Value::Null)
    }

    fn set_bookmark(&self, primary: &str, secondary: Option<&str>, bm: &Value) {
        if let Some(s) = bm.as_str() {
            self.bookmarks
                .lock()
                .unwrap()
                .insert(Self::bm_key(primary, secondary), s.to_string());
        }
    }

    fn bookmark_ended(&self, primary: &str, secondary: Option<&str>) -> bool {
        self.bookmarks
            .lock()
            .unwrap()
            .get(&Self::bm_key(primary, secondary))
            .map(|s| s == "-end-")
            .unwrap_or(false)
    }

    fn ended(batch: &mut Vec<Value>, bm_map: bool) -> bool {
        let _ = bm_map;
        batch.is_empty()
    }

    fn resource_bookmark(resp: &Value) -> Value {
        resp.pointer("/resource/options/bookmarks/0")
            .cloned()
            .unwrap_or(Value::Null)
    }

    fn response_bookmark(resp: &Value) -> Value {
        resp.pointer("/resource_response/bookmark")
            .cloned()
            .unwrap_or(Value::String("-end-".into()))
    }

    fn username_or_self<'a>(&'a self, username: Option<&'a str>) -> &'a str {
        username.filter(|u| !u.is_empty()).unwrap_or(&self.settings.pinterest_username)
    }

    pub fn has_session(&self) -> bool {
        self.cookie("_pinterest_sess").is_some()
    }

    /// HTTP login: warm up cookies, then POST credentials to the user-session
    /// resource. Cookies persist to disk; a session lasts ~15 days.
    pub async fn login(&self) -> Result<Value> {
        if self.settings.pinterest_email.is_empty() || self.settings.pinterest_password.is_empty() {
            return Err(PinError::new(
                401,
                "no pinterest session and PINTEREST_EMAIL/PINTEREST_PASSWORD are not set",
            ));
        }
        // warm-up request so csrftoken/_pinterest_sess cookies exist
        let _ = self
            .http
            .get(HOME_PAGE)
            .headers(self.headers())
            .send()
            .await;
        self.persist_cookies();

        let options = json!({
            "username_or_email": self.settings.pinterest_email,
            "password": self.settings.pinterest_password,
        });
        let body = Self::build_post(options, "/login/?referrer=home_page");
        let resp = self.post(CREATE_USER_SESSION, &body).await?;
        self.persist_cookies();
        if self.has_session() {
            Ok(resp)
        } else {
            let detail = resp
                .pointer("/resource_response/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("login failed (no session cookie set)");
            Err(PinError::new(401, detail))
        }
    }

    pub async fn logout(&self) -> Result<Value> {
        let options = json!({"disable_auth_failure_redirect": true});
        let body = Self::build_data(options);
        self.post(DELETE_USER_SESSION, &body).await
    }

    // ---------- users ----------

    pub async fn user_overview(&self, username: Option<&str>) -> Result<Value> {
        let options = json!({
            "isPrefetch": "false",
            "username": self.username_or_self(username),
            "field_set_key": "profile",
        });
        let url = Self::build_get(USER_RESOURCE, options, "/");
        let resp = self.get(&url).await?;
        Ok(resp.pointer("/resource_response/data").cloned().unwrap_or(Value::Null))
    }

    pub async fn boards(&self, username: Option<&str>, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let username = self.username_or_self(username).to_string();
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("boards", Some(&username)));
            }
            if self.bookmark_ended("boards", Some(&username)) {
                break;
            }
            let options = json!({
                "page_size": page_size,
                "privacy_filter": "all",
                "sort": "custom",
                "username": username,
                "isPrefetch": false,
                "include_archived": true,
                "field_set_key": "profile_grid_item",
                "group_by": "visibility",
                "redux_normalize_feed": true,
                "bookmarks": [self.next_bookmark("boards", Some(&username))],
            });
            let url = Self::build_get(BOARDS_RESOURCE, options, &format!("/{username}/boards/"));
            let resp = self.get(&url).await?;
            self.set_bookmark("boards", Some(&username), &Self::resource_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let mut batch = data.as_array().cloned().unwrap_or_default();
            if Self::ended(&mut batch, false) { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn user_pins(&self, username: Option<&str>, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let username = self.username_or_self(username).to_string();
        let own = username == self.settings.pinterest_username;
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("pins", Some(&username)));
            }
            if self.bookmark_ended("pins", Some(&username)) {
                break;
            }
            let options = json!({
                "username": username,
                "is_own_profile_pins": own,
                "field_set_key": "grid_item",
                "pin_filter": null,
                "bookmarks": [self.next_bookmark("pins", Some(&username))],
                "page_size": page_size,
            });
            let url = Self::build_get(USER_PIN_RESOURCE, options, "/");
            let resp = self.get(&url).await?;
            self.set_bookmark("pins", Some(&username), &Self::resource_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn followers(&self, username: Option<&str>, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let username = self.username_or_self(username).to_string();
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("followers", Some(&username)));
            }
            if self.bookmark_ended("followers", Some(&username)) { break; }
            let options = json!({
                "isPrefetch": false,
                "hide_find_friends_rep": true,
                "username": username,
                "page_size": page_size,
                "bookmarks": [self.next_bookmark("followers", Some(&username))],
            });
            let source = format!("/{}/_followers/", self.settings.pinterest_username);
            let url = Self::build_get(USER_FOLLOWERS_RESOURCE, options, &source);
            let resp = self.get(&url).await?;
            self.set_bookmark("followers", Some(&username), &Self::response_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn following(&self, username: Option<&str>, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let username = self.username_or_self(username).to_string();
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("following", Some(&username)));
            }
            if self.bookmark_ended("following", Some(&username)) { break; }
            let options = json!({
                "isPrefetch": "false",
                "hide_find_friends_rep": "false",
                "username": username,
                "page_size": page_size,
                "bookmarks": [self.next_bookmark("following", Some(&username))],
            });
            let source = format!("/{}/_following/", self.settings.pinterest_email);
            let url = Self::build_get(USER_FOLLOWING_RESOURCE, options, &source);
            let resp = self.get(&url).await?;
            self.set_bookmark("following", Some(&username), &Self::response_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn follow_user(&self, user_id: &str) -> Result<Value> {
        let body = Self::build_post(json!({"user_id": user_id}), "/");
        self.post(FOLLOW_USER_RESOURCE, &body).await
    }

    pub async fn unfollow_user(&self, user_id: &str) -> Result<Value> {
        let body = Self::build_post(json!({"user_id": user_id}), "/");
        self.post(UNFOLLOW_USER_RESOURCE, &body).await
    }

    // ---------- boards ----------

    pub async fn create_board(
        &self,
        name: &str,
        description: &str,
        category: &str,
        privacy: &str,
        layout: &str,
    ) -> Result<Value> {
        let options = json!({
            "name": name,
            "description": description,
            "category": category,
            "privacy": privacy,
            "layout": layout,
            "collab_board_email": "true",
            "collaborator_invites_enabled": "true",
        });
        let source = format!("/{}/boards/", self.settings.pinterest_email);
        let body = Self::build_post(options, &source);
        self.post(CREATE_BOARD_RESOURCE, &body).await
    }

    pub async fn delete_board(&self, board_id: &str) -> Result<Value> {
        let body = Self::build_post(json!({"board_id": board_id}), "/");
        self.post(DELETE_BOARD_RESOURCE, &body).await
    }

    pub async fn follow_board(&self, board_id: &str) -> Result<Value> {
        let body = Self::build_post(json!({"url": format!("/v3/boards/{board_id}/follow/")}), "/");
        self.post(FOLLOW_BOARD_RESOURCE, &body).await
    }

    pub async fn unfollow_board(&self, board_id: &str) -> Result<Value> {
        let body = Self::build_post(json!({"url": format!("/v3/boards/{board_id}/follow/")}), "/");
        self.post(UNFOLLOW_BOARD_RESOURCE, &body).await
    }

    pub async fn board_feed(&self, board_id: &str, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("board_feed", Some(board_id)));
            }
            if self.bookmark_ended("board_feed", Some(board_id)) { break; }
            let options = json!({
                "isPrefetch": false,
                "board_id": board_id,
                "field_set_key": "partner_react_grid_pin",
                "filter_section_pins": true,
                "layout": "default",
                "page_size": page_size,
                "redux_normalize_feed": true,
                "bookmarks": [self.next_bookmark("board_feed", Some(board_id))],
            });
            let url = Self::build_get(BOARD_FEED_RESOURCE, options, "/");
            let mut extra = HeaderMap::new();
            extra.insert("X-Pinterest-PWS-Handler", HeaderValue::from_static("www/[username].js"));
            let resp = self.get_headers(&url, extra).await?;
            self.set_bookmark("board_feed", Some(board_id), &Self::resource_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn board_recommendations(&self, board_id: &str, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("board_recs", Some(board_id)));
            }
            if self.bookmark_ended("board_recs", Some(board_id)) { break; }
            let options = json!({
                "isPrefetch": false,
                "type": "board",
                "id": board_id,
                "page_size": page_size,
                "bookmarks": [self.next_bookmark("board_recs", Some(board_id))],
            });
            let url = Self::build_get(BOARD_RECOMMEND_RESOURCE, options, "/");
            let resp = self.get(&url).await?;
            self.set_bookmark("board_recs", Some(board_id), &Self::resource_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn board_sections(&self, board_id: &str, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("board_sections", Some(board_id)));
            }
            if self.bookmark_ended("board_sections", Some(board_id)) { break; }
            let options = json!({
                "isPrefetch": false,
                "board_id": board_id,
                "page_size": page_size,
                "redux_normalize_feed": true,
                "bookmarks": [self.next_bookmark("board_sections", Some(board_id))],
            });
            let url = Self::build_get(GET_BOARD_SECTIONS, options, "/");
            let resp = self.get(&url).await?;
            self.set_bookmark("board_sections", Some(board_id), &Self::resource_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn create_board_section(&self, board_id: &str, name: &str) -> Result<Value> {
        let options = json!({
            "board_id": board_id,
            "initial_pins": [],
            "name": name,
            "name_source": 0,
        });
        let body = Self::build_post(options, "/");
        self.post(BOARD_SECTION_RESOURCE, &body).await
    }

    pub async fn delete_board_section(&self, section_id: &str) -> Result<Value> {
        let body = Self::build_post(json!({"section_id": section_id}), "/");
        self.post(BOARD_SECTION_EDIT_RESOURCE, &body).await
    }

    pub async fn section_pins(&self, section_id: &str, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("section_pins", Some(section_id)));
            }
            if self.bookmark_ended("section_pins", Some(section_id)) { break; }
            let options = json!({
                "currentFilter": -1,
                "field_set_key": "react_grid_pin",
                "is_own_profile_pins": true,
                "page_size": page_size,
                "redux_normalize_feed": true,
                "section_id": section_id,
                "orbac_subject_id": "",
                "bookmarks": [self.next_bookmark("section_pins", Some(section_id))],
            });
            let url = Self::build_get(GET_BOARD_SECTION_PINS, options, "/");
            let resp = self.get(&url).await?;
            self.set_bookmark("section_pins", Some(section_id), &Self::resource_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch: Vec<Value> = data
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|d| d.get("pinner").is_some())
                .collect();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    // ---------- pins ----------

    pub async fn load_pin(&self, pin_id: &str) -> Result<Value> {
        let options = json!({
            "id": pin_id,
            "field_set_key": "auth_web_main_pin",
            "add_fields": "pin.gen_ai_topics",
            "noCache": true,
            "fetch_visual_search_objects": true,
            "get_page_metadata": false,
        });
        let source = format!("/pin/{pin_id}/");
        let url = Self::build_get(PIN_RESOURCE_GET, options, &source);
        let resp = self.get(&url).await?;
        Ok(resp.pointer("/resource_response/data").cloned().unwrap_or(Value::Null))
    }

    pub async fn create_pin(
        &self,
        board_id: &str,
        image_url: &str,
        description: &str,
        link: &str,
        title: &str,
        alt_text: &str,
        section_id: Option<&str>,
    ) -> Result<Value> {
        let options = json!({
            "board_id": board_id,
            "image_url": image_url,
            "description": description,
            "link": if link.is_empty() { image_url } else { link },
            "scrape_metric": {"source": "www_url_scrape"},
            "method": "uploaded",
            "title": title,
            "alt_text": alt_text,
            "section": section_id,
        });
        let source = format!("/pin/find/?url={}", encode(image_url));
        let body = Self::build_post(options, &source);
        self.post(PIN_RESOURCE_CREATE, &body).await
    }

    pub async fn repin(&self, board_id: &str, pin_id: &str, section_id: Option<&str>) -> Result<Value> {
        let options = json!({
            "board_id": board_id,
            "pin_id": pin_id,
            "section": section_id,
            "is_buyable_pin": false,
        });
        let source = format!("/pin/{pin_id}/");
        let body = Self::build_post(options, &source);
        self.post(REPIN_RESOURCE_CREATE, &body).await
    }

    pub async fn delete_pin(&self, pin_id: &str) -> Result<Value> {
        let source = format!("/{}/", self.settings.pinterest_username);
        let body = Self::build_post(json!({"id": pin_id}), &source);
        self.post(DELETE_PIN_RESOURCE, &body).await
    }

    // ---------- comments ----------

    pub async fn comments(&self, pin_id: &str, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let pin_data = self.load_pin(pin_id).await?;
        let object_id = pin_data
            .pointer("/aggregated_pin_data/id")
            .cloned()
            .unwrap_or(Value::Null);
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("pin_comments", Some(pin_id)));
            }
            if self.bookmark_ended("pin_comments", Some(pin_id)) { break; }
            let options = json!({
                "isPrefetch": false,
                "objectId": object_id,
                "page_size": page_size,
                "redux_normalize_feed": true,
                "bookmarks": [self.next_bookmark("pin_comments", Some(pin_id))],
            });
            let source = format!("/pin/{pin_id}/");
            let url = Self::build_get(GET_PIN_COMMENTS_RESOURCE, options, &source);
            let resp = self.get(&url).await?;
            self.set_bookmark("pin_comments", Some(pin_id), &Self::response_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn comment(&self, pin_id: &str, text: &str) -> Result<Value> {
        let pin_data = self.load_pin(pin_id).await?;
        let object_id = pin_data.pointer("/aggregated_pin_data/id").cloned().unwrap_or(Value::Null);
        let options = json!({
            "objectId": object_id,
            "pinId": pin_id,
            "tags": "[]",
            "text": text,
        });
        let body = Self::build_post(options, pin_id);
        self.post(CREATE_COMMENT_RESOURCE, &body).await
    }

    pub async fn delete_comment(&self, pin_id: &str, comment_id: &str) -> Result<Value> {
        let source = format!("/pin/{pin_id}/");
        let body = Self::build_post(json!({"commentId": comment_id}), &source);
        self.post(DELETE_COMMENT, &body).await
    }

    // ---------- search ----------

    pub async fn search(&self, scope: &str, query: &str, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("search", Some(query)));
            }
            if self.bookmark_ended("search", Some(query)) { break; }
            let terms: Vec<&str> = query.split(' ').collect();
            let escaped = terms.join("%20");
            let term_arg = terms
                .iter()
                .map(|t| format!("term_meta[]={t}"))
                .collect::<Vec<_>>()
                .join("%7Ctyped&");
            let source = format!("/search/{scope}/?q={escaped}&rs=typed&{term_arg}%7Ctyped");
            let options = json!({
                "isPrefetch": false,
                "auto_correction_disabled": false,
                "query": query,
                "redux_normalize_feed": true,
                "rs": "typed",
                "scope": scope,
                "page_size": page_size,
                "bookmarks": [self.next_bookmark("search", Some(query))],
            });
            let url = Self::build_get(BASE_SEARCH_RESOURCE, options, &source);
            let resp = self.get(&url).await?;
            self.set_bookmark("search", Some(query), &Self::resource_bookmark(&resp));
            let data = resp
                .pointer("/resource_response/data/results")
                .cloned()
                .unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn type_ahead(&self, scope: &str, count: u32, term: &str) -> Result<Vec<Value>> {
        let options = json!({
            "pin_scope": scope,
            "count": count,
            "term": term,
            "no_fetch_context_on_resource": false,
        });
        let url = Self::build_get(TYPE_AHEAD_RESOURCE, options, "/");
        let resp = self.get(&url).await?;
        Ok(resp
            .pointer("/resource_response/data/items")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub async fn visual_search(
        &self,
        pin_id: &str,
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
        pages: u32,
    ) -> Result<Vec<Value>> {
        let pin_data = self.load_pin(pin_id).await?;
        let width = pin_data
            .pointer("/images/orig/width")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let height = pin_data
            .pointer("/images/orig/height")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let signature = pin_data
            .pointer("/image_signature")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let padding = 10.0;
        let x = x.unwrap_or(padding);
        let y = y.unwrap_or(padding);
        let w = w.unwrap_or((width - padding * 2.0).max(0.0));
        let h = h.unwrap_or((height - padding * 2.0).max(0.0));
        let source = format!("/pin/{pin_id}/visual-search/?x={x}&y={y}&w={w}&h={h}");
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("visual_search", Some(&source)));
            }
            if self.bookmark_ended("visual_search", Some(&source)) { break; }
            let options = json!({
                "isPrefetch": false,
                "pin_id": pin_id,
                "image_signature": signature,
                "crop": {"x": x / width, "y": y / height, "w": w / width, "h": h / height},
                "bookmarks": [self.next_bookmark("visual_search", Some(&source))],
                "no_fetch_context_on_resource": false,
            });
            let url = Self::build_get(VISUAL_LIVE_SEARCH_RESOURCE, options, &source);
            let resp = self.get(&url).await?;
            self.set_bookmark("visual_search", Some(&source), &Self::resource_bookmark(&resp));
            let data = resp
                .pointer("/resource_response/data/results")
                .cloned()
                .unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn pinnable_images(&self, url: &str) -> Result<Vec<Value>> {
        let options = json!({
            "isPrefetch": "false",
            "url": url,
            "source": "pin_create",
            "appendItems": "false",
            "followRedirects": "true",
        });
        let built = Self::build_get(PINNABLE_IMAGES_RESOURCE, options, "/pin-builder/");
        let resp = self.get(&built).await?;
        let items = resp
            .pointer("/resource_response/data/items")
            .cloned()
            .unwrap_or(json!([]));
        Ok(items
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|i| i.get("url").cloned())
            .collect())
    }

    pub async fn home_feed(&self, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks.lock().unwrap().remove(&Self::bm_key("home_feed", None));
            }
            if self.bookmark_ended("home_feed", None) { break; }
            let options = json!({
                "bookmarks": [self.next_bookmark("home_feed", None)],
                "isPrefetch": false,
                "field_set_key": "hf_grid_partner",
                "in_nux": false,
                "prependPartner": true,
                "prependUserNews": false,
                "static_feed": false,
                "page_size": page_size,
            });
            let url = Self::build_get(USER_HOME_FEED_RESOURCE, options, "/");
            let resp = self.get(&url).await?;
            self.set_bookmark("home_feed", None, &Self::response_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    // ---------- conversations ----------

    pub async fn conversations(&self) -> Result<Vec<Value>> {
        let mut out = vec![];
        loop {
            if self.bookmark_ended("conversations", None) { break; }
            let options = json!({
                "isPrefetch": false,
                "field_set_key": "default",
                "bookmarks": [self.next_bookmark("conversations", None)],
            });
            let url = Self::build_get(CONVERSATION_RESOURCE, options, "/");
            let resp = self.get(&url).await?;
            self.set_bookmark("conversations", None, &Self::resource_bookmark(&resp));
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn initiate_conversation(&self, user_ids: Vec<String>) -> Result<Value> {
        let body = Self::build_post(json!({"user_ids": user_ids, "emails": []}), "/");
        self.post(CONVERSATION_RESOURCE_CREATE, &body).await
    }

    pub async fn conversation_messages(&self, conversation_id: &str, page_size: u32, pages: u32) -> Result<Vec<Value>> {
        let mut out = vec![];
        for i in 0..pages {
            if i == 0 {
                self.bookmarks
                    .lock()
                    .unwrap()
                    .remove(&Self::bm_key("conversation_messages", Some(conversation_id)));
            }
            if self.bookmark_ended("conversation_messages", Some(conversation_id)) { break; }
            let options = json!({
                "isPrefetch": false,
                "page_size": page_size,
                "conversation_id": conversation_id,
                "bookmarks": [self.next_bookmark("conversation_messages", Some(conversation_id))],
            });
            let url = Self::build_get(LOAD_CONVERSATION, options, "/");
            let resp = self.get(&url).await?;
            self.set_bookmark(
                "conversation_messages",
                Some(conversation_id),
                &Self::resource_bookmark(&resp),
            );
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!([]));
            let batch = data.as_array().cloned().unwrap_or_default();
            if batch.is_empty() { break; }
            out.extend(batch);
        }
        Ok(out)
    }

    pub async fn send_message(&self, conversation_id: &str, message: &str, pin_id: &str) -> Result<Value> {
        let mut options = Map::new();
        options.insert("conversation_id".into(), json!(conversation_id));
        options.insert("text".into(), json!(message));
        options.insert("field_set_key".into(), json!(["default_with_reactions"]));
        if !pin_id.is_empty() {
            options.insert("pin".into(), json!(pin_id));
        }
        let body = Self::build_post(Value::Object(options), "/");
        self.post(SEND_MESSAGE, &body).await
    }

    // ---------- media upload (S3 flow) ----------

    async fn api_resource_post(&self, url_path: &str, data: Value) -> Result<Value> {
        let options = json!({"url": url_path, "data": data});
        let body = Self::build_post(options, "/pin-creation-tool/");
        self.post(API_RESOURCE_CREATE, &body).await
    }

    async fn register_media_upload(&self, media_type: &str, duration_ms: Option<i64>) -> Result<Value> {
        let upload_id = uuid::Uuid::new_v4().to_string();
        let mut media_info = json!({"id": upload_id, "media_type": media_type});
        if let Some(d) = duration_ms {
            media_info["upload_aux_data"] = json!({
                "clips": [{"durationMs": d, "isFromImage": false, "startTimestampMs": -1}]
            });
        }
        self.api_resource_post(
            "/v3/media/uploads/register/batch/",
            json!({"media_info_list": serde_json::to_string(&[media_info]).unwrap()}),
        )
        .await
    }

    fn extract_upload_entry(upload_data: &Value) -> Result<&Map<String, Value>> {
        for (_, v) in upload_data.as_object().into_iter().flatten() {
            if let Some(obj) = v.as_object() {
                if obj.contains_key("s3_upload_data") || obj.contains_key("upload_parameters") {
                    return Ok(obj);
                }
            }
        }
        Err(PinError::new(502, format!("no upload entry in registration response: {upload_data}")))
    }

    async fn upload_media_to_s3(
        &self,
        upload_url: &str,
        params: &Map<String, Value>,
        file_path: &Path,
    ) -> Result<()> {
        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let mime = mime_guess::from_path(file_path)
            .first_or_octet_stream()
            .to_string();
        let bytes = tokio::fs::read(file_path)
            .await
            .map_err(|e| PinError::new(400, e.to_string()))?;

        let mut form = reqwest::multipart::Form::new();
        for (k, v) in params {
            if k == "file" {
                continue;
            }
            let text = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            form = form.text(k.clone(), text);
        }
        form = form.part(
            "file",
            reqwest::multipart::Part::bytes(bytes)
                .file_name(file_name)
                .mime_str(&mime)
                .map_err(|e| PinError::new(400, e.to_string()))?,
        );

        let resp = self
            .http
            .post(upload_url)
            .header("Origin", "https://www.pinterest.com")
            .header("Referer", HOME_PAGE)
            .multipart(form)
            .send()
            .await?;
        let status = resp.status().as_u16();
        if ![200, 201, 204].contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            return Err(PinError::new(
                502,
                format!("S3 upload failed: {} {}", status, &text[..text.len().min(300)]),
            ));
        }
        Ok(())
    }

    async fn poll_upload_status(&self, upload_id: &str) -> Result<Map<String, Value>> {
        for _ in 0..30 {
            let options = json!({"upload_ids": [upload_id.to_string()]});
            let url = Self::build_get(VIP_RESOURCE_GET, options, "/pin-creation-tool/");
            let resp = self.get(&url).await?;
            let data = resp.pointer("/resource_response/data").cloned().unwrap_or(json!({}));
            let upload_info = data.get(upload_id).cloned().unwrap_or(data.clone());
            if let Some(info) = upload_info.as_object() {
                let has_result = info.contains_key("signature")
                    || info.contains_key("video_signature")
                    || info.contains_key("image_url");
                if has_result || info.get("status").and_then(|s| s.as_str()) == Some("succeeded") {
                    return Ok(info.clone());
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        Err(PinError::new(504, format!("upload processing timed out for {upload_id}")))
    }

    /// Upload a local image and create a pin via Pinterest's S3 upload flow.
    pub async fn upload_pin(
        &self,
        board_id: &str,
        file_path: &Path,
        description: &str,
        link: &str,
        title: &str,
        alt_text: &str,
        section_id: Option<&str>,
    ) -> Result<Value> {
        let register = self.register_media_upload("image-story-pin", None).await?;
        let upload_data = register.pointer("/resource_response/data").cloned().unwrap_or(json!({}));
        let entry = Self::extract_upload_entry(&upload_data)?;
        let params = entry
            .get("upload_parameters")
            .or_else(|| entry.get("s3_upload_data"))
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let upload_url = entry
            .get("upload_url")
            .and_then(|v| v.as_str())
            .unwrap_or(S3_UPLOAD_URL)
            .to_string();
        let upload_id = entry
            .get("upload_id")
            .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string())))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        self.upload_media_to_s3(&upload_url, &params, file_path).await?;
        let status = self.poll_upload_status(&upload_id).await?;
        let signature = status
            .get("signature")
            .or_else(|| status.get("video_signature"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| PinError::new(502, format!("no signature in upload status: {status:?}")))?;

        let options = json!({
            "board_id": board_id,
            "description": description,
            "link": link,
            "title": title,
            "alt_text": alt_text,
            "section": section_id,
            "upload_id": upload_id.parse::<i64>().unwrap_or_default(),
            "image_signature": signature,
            "method": "uploaded",
            "scrape_metric": {"source": "www_url_scrape"},
        });
        let body = Self::build_post(options, "/pin-creation-tool/");
        self.post(PIN_RESOURCE_CREATE, &body).await
    }

    fn video_info(path: &Path) -> Option<(i64, i64, i64)> {
        let out = std::process::Command::new("ffprobe")
            .args([
                "-v", "quiet", "-print_format", "json", "-show_format", "-show_streams",
            ])
            .arg(path)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let probe: Value = serde_json::from_slice(&out.stdout).ok()?;
        let duration_ms = (probe
            .pointer("/format/duration")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
            * 1000.0) as i64;
        let mut w = 1080i64;
        let mut h = 1920i64;
        for s in probe.pointer("/streams").and_then(|v| v.as_array()).into_iter().flatten() {
            if s.get("codec_type").and_then(|v| v.as_str()) == Some("video") {
                w = s.get("width").and_then(|v| v.as_i64()).unwrap_or(w);
                h = s.get("height").and_then(|v| v.as_i64()).unwrap_or(h);
                break;
            }
        }
        Some((duration_ms, w, h))
    }

    fn extract_cover(video_path: &Path) -> Result<PathBuf> {
        let cover = std::env::temp_dir().join(format!("pinapi-cover-{}.jpg", uuid::Uuid::new_v4()));
        let status = std::process::Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(video_path)
            .args(["-vframes", "1", "-q:v", "2"])
            .arg(&cover)
            .status()
            .map_err(|e| PinError::new(500, e.to_string()))?;
        if !status.success() {
            return Err(PinError::new(500, "failed to extract cover image"));
        }
        Ok(cover)
    }

    /// Upload a video pin (Story/Idea pin) through the S3 flow.
    pub async fn upload_video_pin(
        &self,
        file_path: &Path,
        board_id: Option<&str>,
        title: &str,
        description: &str,
        link: &str,
        alt_text: &str,
        duration_ms: Option<i64>,
        width: Option<i64>,
        height: Option<i64>,
        cover_image: Option<&Path>,
    ) -> Result<Value> {
        let (dur, w, h) = match (duration_ms, width, height) {
            (Some(d), Some(w), Some(h)) => (d, w, h),
            _ => Self::video_info(file_path)
                .ok_or_else(|| PinError::new(400, "could not probe video; install ffprobe or pass duration_ms/width/height"))?,
        };
        let aspect_ratio = 0.56;

        let register = self.register_media_upload("video-story-pin", Some(dur)).await?;
        let upload_data = register.pointer("/resource_response/data").cloned().unwrap_or(json!({}));
        let entry = Self::extract_upload_entry(&upload_data)?;
        let params = entry
            .get("upload_parameters")
            .or_else(|| entry.get("s3_upload_data"))
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let upload_url = entry
            .get("upload_url")
            .and_then(|v| v.as_str())
            .unwrap_or(S3_UPLOAD_URL)
            .to_string();
        let upload_id = entry
            .get("upload_id")
            .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string())))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        self.upload_media_to_s3(&upload_url, &params, file_path).await?;
        let status = self.poll_upload_status(&upload_id).await?;
        let video_signature = status
            .get("signature")
            .or_else(|| status.get("video_signature"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| PinError::new(502, "no video signature in upload status"))?
            .to_string();

        let (cover_path, remove_cover) = match cover_image {
            Some(p) => (p.to_path_buf(), false),
            None => (Self::extract_cover(file_path)?, true),
        };

        let pin_image_signature = async {
            let reg = self.register_media_upload("image-story-pin", None).await?;
            let ud = reg.pointer("/resource_response/data").cloned().unwrap_or(json!({}));
            let e = Self::extract_upload_entry(&ud)?;
            let p = e
                .get("upload_parameters")
                .or_else(|| e.get("s3_upload_data"))
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let u = e
                .get("upload_url")
                .and_then(|v| v.as_str())
                .unwrap_or(S3_UPLOAD_URL)
                .to_string();
            let id = e
                .get("upload_id")
                .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string())))
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            self.upload_media_to_s3(&u, &p, &cover_path).await?;
            let st = self.poll_upload_status(&id).await?;
            st.get("signature")
                .or_else(|| st.get("video_signature"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| PinError::new(502, "no cover signature"))
        }
        .await;
        if remove_cover {
            let _ = std::fs::remove_file(&cover_path);
        }
        let pin_image_signature = pin_image_signature?;

        let story_pin = json!({
            "metadata": {
                "pin_title": title,
                "pin_image_signature": pin_image_signature,
                "canvas_aspect_ratio": aspect_ratio,
            },
            "pages": [{
                "blocks": [{
                    "block_style": {"height": 100, "width": 100, "x_coord": 0, "y_coord": 0},
                    "tracking_id": upload_id,
                    "video_signature": video_signature,
                    "type": 3,
                }],
                "clips": [{
                    "clip_type": 1,
                    "end_time_ms": -1,
                    "is_converted_from_image": false,
                    "source_media_height": h,
                    "source_media_width": w,
                    "start_time_ms": -1,
                }],
                "layout": 0,
                "style": {"background_color": "#FFFFFF"},
            }],
        });

        let mut options = json!({
            "alt_text": alt_text,
            "allow_shopping_rec": true,
            "description": description,
            "is_comments_allowed": true,
            "is_removable": false,
            "is_unified_builder": true,
            "link": link,
            "story_pin": serde_json::to_string(&story_pin).unwrap(),
            "user_mention_tags": "[]",
        });
        if let Some(b) = board_id {
            options["board_id"] = json!(b);
        }
        let body = Self::build_post(options, "/pin-creation-tool/");
        self.post(STORY_PIN_RESOURCE_CREATE, &body).await
    }
}
