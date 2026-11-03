"""pinapi — unofficial Pinterest REST API.

Wraps py3-pinterest (https://github.com/bstoilov/py3-pinterest), which drives
Pinterest's internal web endpoints with a normal account session.
"""

from __future__ import annotations

import asyncio
import tempfile
from pathlib import Path
from typing import Annotated

from fastapi import Depends, FastAPI, File, Form, HTTPException, Query, UploadFile
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
from pydantic import BaseModel, Field

from .client import PinterestClient, PinterestError
from .config import Settings, get_settings

app = FastAPI(title="pinapi", version="0.1.0")

_client: PinterestClient | None = None
_bearer = HTTPBearer(auto_error=False)


def get_client() -> PinterestClient:
    global _client
    if _client is None:
        _client = PinterestClient(get_settings())
    return _client


def require_key(
    creds: Annotated[HTTPAuthorizationCredentials | None, Depends(_bearer)],
    settings: Annotated[Settings, Depends(get_settings)],
) -> None:
    keys = settings.api_keys
    if not keys:
        return  # open mode for self-hosted use
    if creds is None or creds.credentials not in keys:
        raise HTTPException(status_code=401, detail="invalid or missing API key")


async def call(method: str, *args, **kwargs):
    try:
        return await asyncio.to_thread(get_client().call, method, *args, **kwargs)
    except PinterestError as e:
        raise HTTPException(status_code=e.status_code, detail=e.detail) from e


async def call_pages(method: str, pages: int, *args, **kwargs):
    pages = max(1, min(pages, get_settings().pinapi_max_pages))
    try:
        return await asyncio.to_thread(get_client().call_pages, method, pages, *args, **kwargs)
    except PinterestError as e:
        raise HTTPException(status_code=e.status_code, detail=e.detail) from e


async def _save_upload(upload: UploadFile) -> str:
    path = Path(tempfile.mkstemp(dir=_upload_dir(), suffix="-" + (upload.filename or "file"))[1])
    path.write_bytes(await upload.read())
    return str(path)


def _upload_dir() -> str:
    d = Path(get_settings().pinapi_upload_dir)
    d.mkdir(parents=True, exist_ok=True)
    return str(d)


# ---------- models ----------


class PinCreate(BaseModel):
    board_id: str
    image_url: str
    title: str = ""
    description: str = ""
    link: str = ""
    alt_text: str = ""
    section_id: str | None = None


class Repin(BaseModel):
    board_id: str
    section_id: str | None = None


class CommentCreate(BaseModel):
    text: str


class BoardCreate(BaseModel):
    name: str
    description: str = ""
    category: str = "other"
    privacy: str = "public"
    layout: str = "default"


class SectionCreate(BaseModel):
    section_name: str


class ConversationCreate(BaseModel):
    user_ids: list[str]


class MessageCreate(BaseModel):
    message: str = ""
    pin_id: str = ""


class VisualSearch(BaseModel):
    x: int | None = None
    y: int | None = None
    w: int | None = None
    h: int | None = None
    pages: int = 1


# ---------- meta ----------


@app.get("/healthz")
async def healthz():
    return {"status": "ok"}


@app.get("/v1/status", dependencies=[Depends(require_key)])
async def status():
    c = get_client()
    return {
        "logged_in": c.has_session,
        "username": get_settings().pinterest_username or None,
    }


# ---------- search ----------


@app.get("/v1/search", dependencies=[Depends(require_key)])
async def search(
    q: str,
    scope: str = Query("pins", pattern="^(pins|buyable_pins|my_pins|videos|boards)$"),
    pages: int = 1,
    page_size: int = 250,
):
    return await call_pages("search", pages, scope, q, page_size=page_size)


@app.get("/v1/suggest", dependencies=[Depends(require_key)])
async def suggest(term: str, scope: str = "pins", count: int = 5):
    return await call("type_ahead", scope=scope, count=count, term=term)


@app.get("/v1/pinnable", dependencies=[Depends(require_key)])
async def pinnable(url: str):
    return await call("get_pinnable_images", url)


# ---------- pins ----------


@app.get("/v1/pins/{pin_id}", dependencies=[Depends(require_key)])
async def get_pin(pin_id: str):
    return await call("load_pin", pin_id)


@app.post("/v1/pins", dependencies=[Depends(require_key)])
async def create_pin(body: PinCreate):
    return await call("pin", **body.model_dump())


@app.post("/v1/pins/upload", dependencies=[Depends(require_key)])
async def upload_pin(
    file: Annotated[UploadFile, File()],
    board_id: Annotated[str, Form()],
    title: Annotated[str, Form()] = "",
    description: Annotated[str, Form()] = "",
    link: Annotated[str, Form()] = "",
    alt_text: Annotated[str, Form()] = "",
    section_id: Annotated[str | None, Form()] = None,
):
    path = await _save_upload(file)
    return await call(
        "upload_pin",
        board_id=board_id,
        image_file=path,
        title=title,
        description=description,
        link=link,
        alt_text=alt_text,
        section_id=section_id,
    )


@app.post("/v1/pins/upload-video", dependencies=[Depends(require_key)])
async def upload_video_pin(
    file: Annotated[UploadFile, File()],
    board_id: Annotated[str | None, Form()] = None,
    title: Annotated[str, Form()] = "",
    description: Annotated[str, Form()] = "",
    link: Annotated[str, Form()] = "",
    alt_text: Annotated[str, Form()] = "",
):
    path = await _save_upload(file)
    return await call(
        "upload_video_pin",
        video_file=path,
        board_id=board_id,
        title=title,
        description=description,
        link=link,
        alt_text=alt_text,
    )


@app.delete("/v1/pins/{pin_id}", dependencies=[Depends(require_key)])
async def delete_pin(pin_id: str):
    return await call("delete_pin", pin_id)


@app.post("/v1/pins/{pin_id}/repin", dependencies=[Depends(require_key)])
async def repin(pin_id: str, body: Repin):
    return await call("repin", board_id=body.board_id, pin_id=pin_id, section_id=body.section_id)


@app.post("/v1/pins/{pin_id}/visual-search", dependencies=[Depends(require_key)])
async def visual_search(pin_id: str, body: VisualSearch):
    pin = await call("load_pin", pin_id)
    return await call_pages(
        "visual_search", body.pages, pin, x=body.x, y=body.y, w=body.w, h=body.h
    )


# ---------- comments ----------


@app.get("/v1/pins/{pin_id}/comments", dependencies=[Depends(require_key)])
async def get_comments(pin_id: str, pages: int = 1, page_size: int = 50):
    return await call_pages("get_comments", pages, pin_id, page_size=page_size)


@app.post("/v1/pins/{pin_id}/comments", dependencies=[Depends(require_key)])
async def create_comment(pin_id: str, body: CommentCreate):
    return await call("comment", pin_id=pin_id, text=body.text)


@app.delete("/v1/pins/{pin_id}/comments/{comment_id}", dependencies=[Depends(require_key)])
async def delete_comment(pin_id: str, comment_id: str):
    return await call("delete_comment", pin_id=pin_id, comment_id=comment_id)


# ---------- boards ----------


@app.get("/v1/boards", dependencies=[Depends(require_key)])
async def list_boards(username: str | None = None, pages: int = 1, page_size: int = 50):
    return await call_pages("boards", pages, username, page_size=page_size)


@app.post("/v1/boards", dependencies=[Depends(require_key)])
async def create_board(body: BoardCreate):
    return await call("create_board", **body.model_dump())


@app.delete("/v1/boards/{board_id}", dependencies=[Depends(require_key)])
async def delete_board(board_id: str):
    return await call("delete_board", board_id=board_id)


@app.get("/v1/boards/{board_id}/pins", dependencies=[Depends(require_key)])
async def board_feed(board_id: str, pages: int = 1, page_size: int = 250):
    return await call_pages("board_feed", pages, board_id, page_size=page_size)


@app.get("/v1/boards/{board_id}/recommendations", dependencies=[Depends(require_key)])
async def board_recommendations(board_id: str, pages: int = 1, page_size: int = 50):
    return await call_pages("board_recommendations", pages, board_id, page_size=page_size)


@app.get("/v1/boards/{board_id}/sections", dependencies=[Depends(require_key)])
async def board_sections(board_id: str, pages: int = 1, page_size: int = 100):
    return await call_pages("get_board_sections", pages, board_id, page_size=page_size)


@app.post("/v1/boards/{board_id}/sections", dependencies=[Depends(require_key)])
async def create_section(board_id: str, body: SectionCreate):
    return await call("create_board_section", board_id=board_id, section_name=body.section_name)


@app.delete("/v1/sections/{section_id}", dependencies=[Depends(require_key)])
async def delete_section(section_id: str):
    return await call("delete_board_section", section_id=section_id)


@app.get("/v1/sections/{section_id}/pins", dependencies=[Depends(require_key)])
async def section_pins(section_id: str, pages: int = 1, page_size: int = 25):
    return await call_pages("get_section_pins", pages, section_id, page_size=page_size)


@app.post("/v1/boards/{board_id}/follow", dependencies=[Depends(require_key)])
async def follow_board(board_id: str):
    return await call("follow_board", board_id=board_id)


@app.post("/v1/boards/{board_id}/unfollow", dependencies=[Depends(require_key)])
async def unfollow_board(board_id: str):
    return await call("unfollow_board", board_id=board_id)


# ---------- users ----------


@app.get("/v1/users/{username}/overview", dependencies=[Depends(require_key)])
async def user_overview(username: str):
    return await call("get_user_overview", username=username)


@app.get("/v1/users/{username}/pins", dependencies=[Depends(require_key)])
async def user_pins(username: str, pages: int = 1, page_size: int = 250):
    return await call_pages("get_user_pins", pages, username, page_size=page_size)


@app.get("/v1/users/{username}/boards", dependencies=[Depends(require_key)])
async def user_boards(username: str, pages: int = 1, page_size: int = 50):
    return await call_pages("boards", pages, username, page_size=page_size)


@app.get("/v1/users/{username}/followers", dependencies=[Depends(require_key)])
async def user_followers(username: str, pages: int = 1, page_size: int = 250):
    return await call_pages("get_user_followers", pages, username, page_size=page_size)


@app.get("/v1/users/{username}/following", dependencies=[Depends(require_key)])
async def user_following(username: str, pages: int = 1, page_size: int = 250):
    return await call_pages("get_following", pages, username, page_size=page_size)


@app.post("/v1/users/{user_id}/follow", dependencies=[Depends(require_key)])
async def follow_user(user_id: str):
    return await call("follow_user", user_id=user_id)


@app.post("/v1/users/{user_id}/unfollow", dependencies=[Depends(require_key)])
async def unfollow_user(user_id: str):
    return await call("unfollow_user", user_id=user_id)


# ---------- feed ----------


@app.get("/v1/feed", dependencies=[Depends(require_key)])
async def home_feed(pages: int = 1, page_size: int = 100):
    return await call_pages("home_feed", pages, page_size=page_size)


# ---------- messages ----------


@app.get("/v1/conversations", dependencies=[Depends(require_key)])
async def conversations():
    return await call("get_conversations")


@app.post("/v1/conversations", dependencies=[Depends(require_key)])
async def new_conversation(body: ConversationCreate):
    return await call("initiate_conversation", body.user_ids)


@app.get("/v1/conversations/{conversation_id}", dependencies=[Depends(require_key)])
async def conversation(conversation_id: str):
    return await call("load_conversation", conversation_id=conversation_id)


@app.post("/v1/conversations/{conversation_id}/messages", dependencies=[Depends(require_key)])
async def send_message(conversation_id: str, body: MessageCreate):
    return await call(
        "send_message",
        conversation_id=conversation_id,
        message=body.message,
        pin_id=body.pin_id,
    )
