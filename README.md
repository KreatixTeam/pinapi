# pinapi

Unofficial REST API for Pinterest, written in Rust (axum + reqwest). It is a port of the [py3-pinterest](https://github.com/bstoilov/py3-pinterest) client's internals (MIT) — it drives Pinterest's internal web endpoints using a normal account session, so **no official API approval is needed**.

> Unofficial. Use a dedicated account; automated use may violate Pinterest ToS.

## Run

```bash
cp .env.example .env        # fill in PINTEREST_* and PINAPI_API_KEYS
docker compose up -d --build
```

Or locally:

```bash
cargo build --release
./target/release/pinapi    # listens on PINAPI_BIND (default 0.0.0.0:8000)
```

Health at `/healthz`, session state at `/v1/status`.

## Auth

Two layers:

1. **Pinterest session** — all read endpoints (search, pins, boards, users, suggestions) work **without credentials**; Pinterest serves public data anonymously. Write operations (create/delete pins, boards, follow, comment) and personal data (home feed, conversations) need an account: set `PINTEREST_EMAIL`/`PINTEREST_PASSWORD`/`PINTEREST_USERNAME` and call `POST /v1/login`, or seed `PINTEREST_COOKIES` with a `_pinterest_sess` cookie. Cookies persist in `PINAPI_CRED_ROOT` (sessions last ~15 days).
2. **Client auth** — if `PINAPI_API_KEYS` is set, every `/v1/*` route requires `Authorization: Bearer <key>`. Empty means open access (dev only).

## Endpoints

All list endpoints accept `pages` (1–`PINAPI_MAX_PAGES`, default 10) and `page_size`.

### Meta
| Method | Path | Notes |
|---|---|---|
| GET | `/healthz` | liveness |
| GET | `/v1/status` | logged_in, username |
| POST | `/v1/login` | logs in with `PINTEREST_*` env creds |
| POST | `/v1/logout` | ends the session |

### Search
| Method | Path | Notes |
|---|---|---|
| GET | `/v1/search?q=&scope=pins&pages=&page_size=` | scopes: `pins`, `buyable_pins`, `my_pins`, `videos`, `boards` |
| GET | `/v1/suggest?term=&scope=pins&count=5` | type-ahead suggestions |
| GET | `/v1/pinnable?url=` | pinnable images found on any URL |
| POST | `/v1/pins/{pin_id}/visual-search` | `{x,y,w,h,pages}` crop search |

### Pins
| Method | Path | Notes |
|---|---|---|
| GET | `/v1/pins/{pin_id}` | full pin data |
| POST | `/v1/pins` | `{board_id, image_url, title?, description?, link?, alt_text?, section_id?}` |
| POST | `/v1/pins/upload` | multipart: `file`, `board_id`, `title`… |
| POST | `/v1/pins/upload-video` | multipart video pin (needs ffmpeg) |
| POST | `/v1/pins/{pin_id}/repin` | `{board_id, section_id?}` |
| DELETE | `/v1/pins/{pin_id}` | |

### Comments
| Method | Path | Notes |
|---|---|---|
| GET | `/v1/pins/{pin_id}/comments?pages=` | upstream resource currently 404s |
| POST | `/v1/pins/{pin_id}/comments` | `{text}` |
| DELETE | `/v1/pins/{pin_id}/comments/{comment_id}` | |

### Boards
| Method | Path | Notes |
|---|---|---|
| GET | `/v1/boards?username=&pages=` | own boards when username omitted |
| POST | `/v1/boards` | `{name, description?, category?, privacy?, layout?}` |
| DELETE | `/v1/boards/{board_id}` | |
| GET | `/v1/boards/{board_id}/pins?pages=` | board feed |
| GET | `/v1/boards/{board_id}/recommendations` | "more ideas" |
| GET | `/v1/boards/{board_id}/sections` | |
| POST | `/v1/boards/{board_id}/sections` | `{section_name}` |
| GET | `/v1/sections/{section_id}/pins` | |
| DELETE | `/v1/sections/{section_id}` | |
| POST | `/v1/boards/{board_id}/follow` · `/unfollow` | |

### Users
| Method | Path | Notes |
|---|---|---|
| GET | `/v1/users/{username}/overview` | profile |
| GET | `/v1/users/{username}/pins` · `/boards` | |
| GET | `/v1/users/{username}/followers` · `/following` | requires login |
| POST | `/v1/users/{user_id}/follow` · `/unfollow` | |

### Feed & messages
| Method | Path | Notes |
|---|---|---|
| GET | `/v1/feed?pages=` | home feed (login required) |
| GET | `/v1/conversations` | login required |
| POST | `/v1/conversations` | `{user_ids: [...]}` |
| GET | `/v1/conversations/{id}` | |
| POST | `/v1/conversations/{id}/messages` | `{message?, pin_id?}` |

## Example

```bash
curl -H "Authorization: Bearer $KEY" \
  "http://localhost:8000/v1/search?q=interior%20design&scope=pins&page_size=50"
```

Responses are slimmed by default: pins → `id`, `type`, `title`, `description`, `link`, `domain`, `created_at`, `image`, `images` (`orig`/`736x`/`236x`), `video`, `board`, `pinner`; boards → `id`, `name`, `description`, `image`, `pin_count`, `owner`; users → `id`, `username`, `full_name`, `image`, `follower_count`, `about`. Grid/ad modules are filtered out and internal/session fields are dropped. Send header `x-raw: 1` for the full upstream payload.

## Notes

- The `data` payload must be serialized with Python `json.dumps` default separators (`", "` / `": "`) — Pinterest's resource endpoints reject compact JSON, so the client mirrors that format exactly.
- Every resource `GET` sends `X-Pinterest-PWS-Handler: www/[username].js`, which Pinterest requires.
- A few upstream resources (`AggregatedCommentFeedResource`, `VisualLiveSearchResource`, `FindPinImagesResource`) are dead in py3-pinterest too — the endpoints proxy whatever Pinterest returns.

## Config

See `.env.example`.
