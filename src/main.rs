mod config;
mod pinterest;

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::Settings;
use crate::pinterest::{PinError, Pinterest};

struct AppState {
    pin: Pinterest,
    settings: Settings,
    limits: std::sync::Mutex<std::collections::HashMap<String, (u32, std::time::Instant)>>,
}

impl From<PinError> for (StatusCode, Json<Value>) {
    fn from(e: PinError) -> Self {
        (
            StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(json!({"detail": e.detail})),
        )
    }
}

type ApiResult<T> = Result<T, (StatusCode, Json<Value>)>;

fn check_auth(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    if state.settings.api_keys.is_empty() {
        return Ok(());
    }
    let ok = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| state.settings.api_keys.iter().any(|k| k == t))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, Json(json!({"detail": "invalid or missing API key"}))))
    }
}

#[derive(Deserialize)]
struct Paged {
    pages: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Deserialize)]
struct SearchQ {
    q: String,
    scope: Option<String>,
    pages: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Deserialize)]
struct BoardsQ {
    username: Option<String>,
    pages: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Deserialize)]
struct SuggestQ {
    term: String,
    scope: Option<String>,
    count: Option<u32>,
}

#[derive(Deserialize)]
struct PinnableQ {
    url: String,
}

#[derive(Deserialize)]
struct PinCreate {
    board_id: String,
    image_url: String,
    title: Option<String>,
    description: Option<String>,
    link: Option<String>,
    alt_text: Option<String>,
    section_id: Option<String>,
}

#[derive(Deserialize)]
struct RepinBody {
    board_id: String,
    section_id: Option<String>,
}

#[derive(Deserialize)]
struct CommentBody {
    text: String,
}

#[derive(Deserialize)]
struct BoardBody {
    name: String,
    description: Option<String>,
    category: Option<String>,
    privacy: Option<String>,
    layout: Option<String>,
}

#[derive(Deserialize)]
struct SectionBody {
    section_name: String,
}

#[derive(Deserialize)]
struct VisualBody {
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    pages: Option<u32>,
}

#[derive(Deserialize)]
struct ConversationBody {
    user_ids: Vec<String>,
}

#[derive(Deserialize)]
struct MessageBody {
    message: Option<String>,
    pin_id: Option<String>,
}

fn clamp_pages(state: &AppState, pages: Option<u32>) -> u32 {
    pages.unwrap_or(1).clamp(1, state.settings.max_pages)
}

async fn save_upload(state: &AppState, file: axum::extract::multipart::Field<'_>) -> ApiResult<PathBuf> {
    let dir = PathBuf::from(&state.settings.upload_dir);
    let _ = tokio::fs::create_dir_all(&dir).await;
    let name = file
        .file_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("upload-{}", uuid::Uuid::new_v4()));
    let path = dir.join(format!("{}-{}", uuid::Uuid::new_v4(), name));
    let data = file
        .bytes()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"detail": e.to_string()}))))?;
    tokio::fs::write(&path, &data)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"detail": e.to_string()}))))?;
    Ok(path)
}

fn form_str(field: &axum::extract::multipart::Field<'_>) -> String {
    field.name().unwrap_or_default().to_string()
}

// ---------- response slimming ----------

fn pick(v: &Value, path: &str) -> Value {
    v.pointer(path).cloned().unwrap_or(Value::Null)
}

fn slim_pin(p: &Value) -> Value {
    let mut imgs = serde_json::Map::new();
    if let Some(images) = p.get("images").and_then(|i| i.as_object()) {
        for k in ["orig", "736x", "236x"] {
            if let Some(v) = images.get(k) {
                imgs.insert(k.to_string(), v.clone());
            }
        }
    }
    let video = pick(p, "/videos/video_list/V_720P/url");
    json!({
        "id": pick(p, "/id"),
        "type": pick(p, "/type"),
        "title": pick(p, "/title"),
        "description": pick(p, "/description"),
        "link": pick(p, "/link"),
        "domain": pick(p, "/domain"),
        "created_at": pick(p, "/created_at"),
        "image": pick(p, "/images/orig/url"),
        "images": Value::Object(imgs),
        "video": video,
        "board": {
            "id": pick(p, "/board/id"),
            "name": pick(p, "/board/name"),
            "url": pick(p, "/board/url"),
        },
        "pinner": {
            "username": pick(p, "/pinner/username"),
            "full_name": pick(p, "/pinner/full_name"),
            "image": pick(p, "/pinner/image_xlarge_url"),
        },
    })
}

fn slim_board(b: &Value) -> Value {
    json!({
        "id": pick(b, "/id"),
        "name": pick(b, "/name"),
        "description": pick(b, "/description"),
        "url": pick(b, "/url"),
        "image": pick(b, "/image_cover_url"),
        "pin_count": pick(b, "/pin_count"),
        "privacy": pick(b, "/privacy"),
        "owner": {
            "username": pick(b, "/owner/username"),
            "full_name": pick(b, "/owner/full_name"),
        },
    })
}

fn slim_user(u: &Value) -> Value {
    json!({
        "id": pick(u, "/id"),
        "username": pick(u, "/username"),
        "full_name": pick(u, "/full_name"),
        "image": pick(u, "/image_xlarge_url"),
        "follower_count": pick(u, "/follower_count"),
        "url": pick(u, "/url"),
        "about": pick(u, "/about"),
    })
}

fn slim_comment(c: &Value) -> Value {
    json!({
        "id": pick(c, "/id"),
        "text": pick(c, "/comment"),
        "created_at": pick(c, "/created_at"),
        "commenter": {
            "username": pick(c, "/commenter/username"),
            "full_name": pick(c, "/commenter/full_name"),
        },
    })
}

fn slim(v: Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(
            a.into_iter()
                // drop grid/ad modules Pinterest injects into feeds
                .filter(|x| {
                    !(x.is_object()
                        && (x.get("container_type").is_some()
                            || x.get("is_promoted").and_then(|p| p.as_bool()) == Some(true)))
                })
                .map(slim)
                .collect(),
        ),
        Value::Object(o) => {
            let is_board = o.contains_key("privacy") && o.contains_key("owner");
            let is_pin = !is_board && o.contains_key("images") && o.contains_key("id");
            let is_user = o.contains_key("username")
                && (o.contains_key("image_xlarge_url") || o.contains_key("follower_count"));
            let is_comment = o.contains_key("comment") && o.contains_key("commenter");
            if is_pin {
                slim_pin(&Value::Object(o))
            } else if is_board {
                slim_board(&Value::Object(o))
            } else if is_user {
                slim_user(&Value::Object(o))
            } else if is_comment {
                slim_comment(&Value::Object(o))
            } else {
                Value::Object(o.into_iter().map(|(k, x)| (k, slim(x))).collect())
            }
        }
        other => other,
    }
}

/// Slims every JSON response; `x-raw: 1` opts out.
async fn slim_mw(req: Request<Body>, next: Next) -> Response {
    let raw = req
        .headers()
        .get("x-raw")
        .and_then(|v| v.to_str().ok())
        == Some("1");
    let resp = next.run(req).await;
    if raw {
        return resp;
    }
    let (parts, body) = resp.into_parts();
    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
        return Response::from_parts(parts, Body::empty());
    };
    let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    let bytes = serde_json::to_vec(&slim(v)).unwrap_or_default();
    Response::from_parts(parts, Body::from(bytes))
}

/// Bearer-key gate for write/personal routes; open when PINAPI_API_KEYS is empty.
async fn auth_mw(State(s): State<Arc<AppState>>, req: Request<Body>, next: Next) -> Response {
    if s.settings.api_keys.is_empty() {
        return next.run(req).await;
    }
    if check_auth(&s, req.headers()).is_ok() {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"detail": "invalid or missing API key"})),
        )
            .into_response()
    }
}

/// 60 req/min per client IP on public routes (cf-connecting-ip → x-forwarded-for).
async fn rate_mw(State(s): State<Arc<AppState>>, req: Request<Body>, next: Next) -> Response {
    let ip = req
        .headers()
        .get("cf-connecting-ip")
        .or_else(|| req.headers().get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').next().unwrap_or("").trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "anon".to_string());
    let now = std::time::Instant::now();
    {
        let mut lim = s.limits.lock().unwrap();
        let e = lim.entry(ip).or_insert((0, now));
        if now.duration_since(e.1).as_secs() >= 60 {
            *e = (0, now);
        }
        e.0 += 1;
        if e.0 > 60 {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"detail": "rate limited"})),
            )
                .into_response();
        }
    }
    next.run(req).await
}

// ---------- meta ----------

async fn healthz() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

async fn docs() -> impl IntoResponse {
    axum::response::Html(include_str!("../docs/index.html"))
}

async fn index() -> impl IntoResponse {
    Json(json!({
        "service": "pinapi",
        "version": env!("CARGO_PKG_VERSION"),
        "docs": "https://github.com/XFlexDev/pinapi",
        "endpoints": [
            "GET /healthz",
            "GET /v1/status",
            "POST /v1/login",
            "POST /v1/logout",
            "GET /v1/search?q=&scope=pins",
            "GET /v1/suggest?term=",
            "GET /v1/pinnable?url=",
            "GET /v1/pins/{id}",
            "POST /v1/pins",
            "POST /v1/pins/upload",
            "POST /v1/pins/upload-video",
            "DELETE /v1/pins/{id}",
            "POST /v1/pins/{id}/repin",
            "POST /v1/pins/{id}/visual-search",
            "GET|POST /v1/pins/{id}/comments",
            "DELETE /v1/pins/{pin_id}/comments/{comment_id}",
            "GET|POST /v1/boards",
            "DELETE /v1/boards/{id}",
            "GET /v1/boards/{id}/pins",
            "GET /v1/boards/{id}/recommendations",
            "GET|POST /v1/boards/{id}/sections",
            "POST /v1/boards/{id}/follow | /unfollow",
            "GET /v1/sections/{id}/pins",
            "DELETE /v1/sections/{id}",
            "GET /v1/users/{username}/overview | /pins | /boards | /followers | /following",
            "POST /v1/users/{user_id}/follow | /unfollow",
            "GET /v1/feed",
            "GET|POST /v1/conversations",
            "GET /v1/conversations/{id}",
            "POST /v1/conversations/{id}/messages",
        ],
    }))
}

async fn status(State(s): State<Arc<AppState>>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(json!({
        "logged_in": s.pin.has_session(),
        "username": if s.settings.pinterest_username.is_empty() { Value::Null } else { json!(s.settings.pinterest_username) },
    })))
}

async fn login(State(s): State<Arc<AppState>>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.login().await?))
}

async fn logout(State(s): State<Arc<AppState>>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.logout().await?))
}

// ---------- search ----------

async fn search(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SearchQ>,
) -> ApiResult<impl IntoResponse> {
    let scope = q.scope.unwrap_or_else(|| "pins".into());
    if !["pins", "buyable_pins", "my_pins", "videos", "boards"].contains(&scope.as_str()) {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"detail": "invalid scope"}))));
    }
    Ok(Json(s.pin.search(&scope, &q.q, q.page_size.unwrap_or(250), clamp_pages(&s, q.pages)).await?))
}

async fn suggest(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SuggestQ>,
) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.type_ahead(&q.scope.unwrap_or_else(|| "pins".into()), q.count.unwrap_or(5), &q.term).await?))
}

async fn pinnable(
    State(s): State<Arc<AppState>>,
    Query(q): Query<PinnableQ>,
) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.pinnable_images(&q.url).await?))
}

// ---------- pins ----------

async fn get_pin(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.load_pin(&id).await?))
}

async fn create_pin(State(s): State<Arc<AppState>>, headers: HeaderMap, Json(b): Json<PinCreate>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.create_pin(
        &b.board_id,
        &b.image_url,
        &b.description.unwrap_or_default(),
        &b.link.unwrap_or_default(),
        &b.title.unwrap_or_default(),
        &b.alt_text.unwrap_or_default(),
        b.section_id.as_deref(),
    ).await?))
}

async fn upload_pin(State(s): State<Arc<AppState>>, headers: HeaderMap, mut mp: Multipart) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    let mut fields: std::collections::HashMap<String, String> = Default::default();
    let mut file_path: Option<PathBuf> = None;
    while let Ok(Some(field)) = mp.next_field().await {
        if field.name() == Some("file") {
            file_path = Some(save_upload(&s, field).await?);
        } else {
            let key = form_str(&field);
            if let Ok(v) = field.text().await {
                fields.insert(key, v);
            }
        }
    }
    let path = file_path.ok_or((StatusCode::BAD_REQUEST, Json(json!({"detail": "missing file"}))))?;
    let board_id = fields.get("board_id").cloned().unwrap_or_default();
    Ok(Json(s.pin.upload_pin(
        &board_id,
        &path,
        &fields.get("description").cloned().unwrap_or_default(),
        &fields.get("link").cloned().unwrap_or_default(),
        &fields.get("title").cloned().unwrap_or_default(),
        &fields.get("alt_text").cloned().unwrap_or_default(),
        fields.get("section_id").map(|s| s.as_str()),
    ).await?))
}

async fn upload_video(State(s): State<Arc<AppState>>, headers: HeaderMap, mut mp: Multipart) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    let mut fields: std::collections::HashMap<String, String> = Default::default();
    let mut file_path: Option<PathBuf> = None;
    while let Ok(Some(field)) = mp.next_field().await {
        if field.name() == Some("file") {
            file_path = Some(save_upload(&s, field).await?);
        } else {
            let key = form_str(&field);
            if let Ok(v) = field.text().await {
                fields.insert(key, v);
            }
        }
    }
    let path = file_path.ok_or((StatusCode::BAD_REQUEST, Json(json!({"detail": "missing file"}))))?;
    let opt = |k: &str| fields.get(k).cloned().filter(|v| !v.is_empty());
    Ok(Json(s.pin.upload_video_pin(
        &path,
        opt("board_id").as_deref(),
        &fields.get("title").cloned().unwrap_or_default(),
        &fields.get("description").cloned().unwrap_or_default(),
        &fields.get("link").cloned().unwrap_or_default(),
        &fields.get("alt_text").cloned().unwrap_or_default(),
        opt("duration_ms").and_then(|v| v.parse().ok()),
        opt("width").and_then(|v| v.parse().ok()),
        opt("height").and_then(|v| v.parse().ok()),
        None,
    ).await?))
}

async fn delete_pin(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.delete_pin(&id).await?))
}

async fn repin(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>, Json(b): Json<RepinBody>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.repin(&b.board_id, &id, b.section_id.as_deref()).await?))
}

async fn visual_search(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(id): Path<String>, Json(b): Json<VisualBody>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.visual_search(&id, b.x, b.y, b.w, b.h, clamp_pages(&s, b.pages)).await?))
}

// ---------- comments ----------

async fn get_comments(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(id): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.comments(&id, p.page_size.unwrap_or(50), clamp_pages(&s, p.pages)).await?))
}

async fn create_comment(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>, Json(b): Json<CommentBody>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.comment(&id, &b.text).await?))
}

async fn delete_comment(State(s): State<Arc<AppState>>, headers: HeaderMap, Path((pid, cid)): Path<(String, String)>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.delete_comment(&pid, &cid).await?))
}

// ---------- boards ----------

async fn list_boards(State(s): State<Arc<AppState>>, _headers: HeaderMap, Query(q): Query<BoardsQ>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.boards(q.username.as_deref(), q.page_size.unwrap_or(50), clamp_pages(&s, q.pages)).await?))
}

async fn create_board(State(s): State<Arc<AppState>>, headers: HeaderMap, Json(b): Json<BoardBody>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.create_board(
        &b.name,
        &b.description.unwrap_or_default(),
        &b.category.unwrap_or_else(|| "other".into()),
        &b.privacy.unwrap_or_else(|| "public".into()),
        &b.layout.unwrap_or_else(|| "default".into()),
    ).await?))
}

async fn delete_board(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.delete_board(&id).await?))
}

async fn board_feed(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(id): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.board_feed(&id, p.page_size.unwrap_or(250), clamp_pages(&s, p.pages)).await?))
}

async fn board_recs(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(id): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.board_recommendations(&id, p.page_size.unwrap_or(50), clamp_pages(&s, p.pages)).await?))
}

async fn board_sections(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(id): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.board_sections(&id, p.page_size.unwrap_or(100), clamp_pages(&s, p.pages)).await?))
}

async fn create_section(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>, Json(b): Json<SectionBody>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.create_board_section(&id, &b.section_name).await?))
}

async fn delete_section(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.delete_board_section(&id).await?))
}

async fn section_pins(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(id): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.section_pins(&id, p.page_size.unwrap_or(25), clamp_pages(&s, p.pages)).await?))
}

async fn follow_board(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.follow_board(&id).await?))
}

async fn unfollow_board(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.unfollow_board(&id).await?))
}

// ---------- users ----------

async fn user_overview(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(u): Path<String>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.user_overview(Some(&u)).await?))
}

async fn user_pins(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(u): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.user_pins(Some(&u), p.page_size.unwrap_or(250), clamp_pages(&s, p.pages)).await?))
}

async fn user_boards(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(u): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.boards(Some(&u), p.page_size.unwrap_or(50), clamp_pages(&s, p.pages)).await?))
}

async fn user_followers(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(u): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.followers(Some(&u), p.page_size.unwrap_or(250), clamp_pages(&s, p.pages)).await?))
}

async fn user_following(State(s): State<Arc<AppState>>, _headers: HeaderMap, Path(u): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    Ok(Json(s.pin.following(Some(&u), p.page_size.unwrap_or(250), clamp_pages(&s, p.pages)).await?))
}

async fn follow_user(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.follow_user(&id).await?))
}

async fn unfollow_user(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.unfollow_user(&id).await?))
}

// ---------- feed & messages ----------

async fn home_feed(State(s): State<Arc<AppState>>, headers: HeaderMap, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.home_feed(p.page_size.unwrap_or(100), clamp_pages(&s, p.pages)).await?))
}

async fn conversations(State(s): State<Arc<AppState>>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.conversations().await?))
}

async fn new_conversation(State(s): State<Arc<AppState>>, headers: HeaderMap, Json(b): Json<ConversationBody>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.initiate_conversation(b.user_ids).await?))
}

async fn conversation(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>, Query(p): Query<Paged>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.conversation_messages(&id, p.page_size.unwrap_or(25), clamp_pages(&s, p.pages)).await?))
}

async fn send_message(State(s): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>, Json(b): Json<MessageBody>) -> ApiResult<impl IntoResponse> {
    check_auth(&s, &headers)?;
    Ok(Json(s.pin.send_message(&id, &b.message.unwrap_or_default(), &b.pin_id.unwrap_or_default()).await?))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let settings = Settings::from_env();
    let pin = Pinterest::new(&settings).unwrap_or_else(|e| {
        eprintln!("failed to init pinterest client: {e}");
        std::process::exit(1);
    });
    let state = Arc::new(AppState {
        pin,
        settings,
        limits: std::sync::Mutex::new(std::collections::HashMap::new()),
    });

    // public read endpoints — no key, rate limited
    let public = Router::new()
        .route("/v1/search", get(search))
        .route("/v1/suggest", get(suggest))
        .route("/v1/pinnable", get(pinnable))
        .route("/v1/pins/{id}/visual-search", post(visual_search))
        .route("/v1/pins/{id}", get(get_pin))
        .route("/v1/pins/{id}/comments", get(get_comments))
        .route("/v1/boards/{id}/pins", get(board_feed))
        .route("/v1/boards/{id}/recommendations", get(board_recs))
        .route("/v1/boards/{id}/sections", get(board_sections))
        .route("/v1/sections/{id}/pins", get(section_pins))
        .route("/v1/users/{u}/overview", get(user_overview))
        .route("/v1/users/{u}/pins", get(user_pins))
        .route("/v1/users/{u}/boards", get(user_boards))
        .route("/v1/users/{u}/followers", get(user_followers))
        .route("/v1/users/{u}/following", get(user_following))
        .route_layer(middleware::from_fn_with_state(state.clone(), rate_mw));

    // write + personal endpoints — Bearer key required when PINAPI_API_KEYS is set
    let protected = Router::new()
        .route("/v1/status", get(status))
        .route("/v1/login", post(login))
        .route("/v1/logout", post(logout))
        .route("/v1/pins/{id}", delete(delete_pin))
        .route("/v1/pins", post(create_pin))
        .route("/v1/pins/upload", post(upload_pin))
        .route("/v1/pins/upload-video", post(upload_video))
        .route("/v1/pins/{id}/repin", post(repin))
        .route("/v1/pins/{id}/comments", post(create_comment))
        .route("/v1/pins/{id}/comments/{cid}", delete(delete_comment))
        .route("/v1/boards", get(list_boards).post(create_board))
        .route("/v1/boards/{id}", delete(delete_board))
        .route("/v1/boards/{id}/sections", post(create_section))
        .route("/v1/boards/{id}/follow", post(follow_board))
        .route("/v1/boards/{id}/unfollow", post(unfollow_board))
        .route("/v1/sections/{id}", delete(delete_section))
        .route("/v1/users/{u}/follow", post(follow_user))
        .route("/v1/users/{u}/unfollow", post(unfollow_user))
        .route("/v1/feed", get(home_feed))
        .route("/v1/conversations", get(conversations).post(new_conversation))
        .route("/v1/conversations/{id}", get(conversation))
        .route("/v1/conversations/{id}/messages", post(send_message))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_mw));

    let app = Router::new()
        .route("/", get(index))
        .route("/docs", get(docs))
        .route("/healthz", get(healthz))
        .merge(public)
        .merge(protected)
        .layer(middleware::from_fn(slim_mw))
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024))
        .with_state(state.clone());

    let bind = state.settings.bind.clone();
    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap_or_else(|e| {
        eprintln!("bind {bind}: {e}");
        std::process::exit(1);
    });
    println!("pinapi listening on {bind}");
    axum::serve(listener, app).await.unwrap();
}
