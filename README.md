# pinapi

Unofficial REST API for Pinterest, built on [py3-pinterest](https://github.com/bstoilov/py3-pinterest) — the most complete, actively maintained open-source Pinterest client (MIT). It drives Pinterest's internal web endpoints using a normal account session, so **no official API approval is needed**.

> Unofficial. Use a dedicated account; automated use may violate Pinterest ToS.

## Run

```bash
cp .env.example .env        # fill in PINTEREST_* and PINAPI_API_KEYS
docker compose up -d --build
```

Or locally:

```bash
pip install -r requirements.txt
uvicorn pinapi.main:app --port 8000
```

Interactive docs at `/docs` (Swagger UI), health at `/healthz`, session state at `/v1/status`.

## Auth

Two layers:

1. **Pinterest session** — all read endpoints (search, pins, boards, users, comments, suggestions) work **without credentials**; Pinterest serves public data anonymously. Write operations (create/delete pins, boards, follow, comment) and personal data (home feed, conversations) need an account: set `PINTEREST_EMAIL`/`PINTEREST_PASSWORD`/`PINTEREST_USERNAME` and the service logs in once via headless Chrome, caching cookies in `PINAPI_CRED_ROOT` (sessions last ~15 days, auto-relogin on 401/403).
2. **Client auth** — if `PINAPI_API_KEYS` is set, every `/v1/*` route requires `Authorization: Bearer <key>`. Empty means open access (dev only).

## Endpoints

All list endpoints accept `pages` (1–`PINAPI_MAX_PAGES`, default 10) and `page_size`.

### Meta
| Method | Path | Notes |
|---|---|---|
| GET | `/healthz` | liveness |
| GET | `/v1/status` | logged_in, username |

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
| GET | `/v1/pins/{pin_id}/comments?pages=` | |
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
| GET | `/v1/users/{username}/followers` · `/following` | |
| POST | `/v1/users/{user_id}/follow` · `/unfollow` | |

### Feed & messages
| Method | Path | Notes |
|---|---|---|
| GET | `/v1/feed?pages=` | home feed |
| GET | `/v1/conversations` | |
| POST | `/v1/conversations` | `{user_ids: [...]}` |
| GET | `/v1/conversations/{id}` | |
| POST | `/v1/conversations/{id}/messages` | `{message?, pin_id?}` |

## Example

```bash
curl -H "Authorization: Bearer $KEY" \
  "http://localhost:8000/v1/search?q=interior%20design&scope=pins&page_size=50"
```

Responses are Pinterest's raw `resource_response` payloads — pin objects carry `id`, `images` (`orig`, `236x`, …), `link`, `title`, `description`, `pinner`, `board`, etc.

## Tests

```bash
pip install pytest httpx python-multipart
pytest tests
```

## Config

See `.env.example`. `PINAPI_CHROME_NO_SANDBOX` / `PINAPI_CHROME_BINARY` / `PINAPI_CHROMEDRIVER_PATH` tune the login browser for containers; the Dockerfile already handles it.
