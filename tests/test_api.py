import pytest
from fastapi.testclient import TestClient

import pinapi.main as main


class FakeClient:
    def __init__(self):
        self.calls = []

    @property
    def has_session(self):
        return True

    def call(self, method, *args, **kwargs):
        self.calls.append((method, args, kwargs))
        return {"method": method, "args": list(args), "kwargs": kwargs}

    def call_pages(self, method, pages, *args, **kwargs):
        self.calls.append((method, args, kwargs, pages))
        return [{"method": method, "page": i} for i in range(pages)]


@pytest.fixture()
def client(monkeypatch, tmp_path):
    fake = FakeClient()
    monkeypatch.setattr(main, "_client", fake)
    monkeypatch.setenv("PINAPI_UPLOAD_DIR", str(tmp_path))
    return TestClient(main.app), fake


def test_healthz(client):
    c, _ = client
    assert c.get("/healthz").json() == {"status": "ok"}


def test_status(client):
    c, _ = client
    r = c.get("/v1/status")
    assert r.status_code == 200
    assert r.json()["logged_in"] is True


def test_search(client):
    c, fake = client
    r = c.get("/v1/search", params={"q": "cats", "scope": "pins", "pages": 2})
    assert r.status_code == 200
    assert len(r.json()) == 2
    assert fake.calls[0][0] == "search"
    assert fake.calls[0][1] == ("pins", "cats")


def test_search_bad_scope(client):
    c, _ = client
    assert c.get("/v1/search", params={"q": "x", "scope": "nope"}).status_code == 422


def test_get_pin(client):
    c, fake = client
    r = c.get("/v1/pins/123")
    assert r.json()["method"] == "load_pin"
    assert fake.calls[0][1] == ("123",)


def test_create_pin(client):
    c, fake = client
    r = c.post(
        "/v1/pins",
        json={"board_id": "b1", "image_url": "https://x/i.jpg", "title": "hi"},
    )
    assert r.status_code == 200
    assert fake.calls[0][2]["board_id"] == "b1"
    assert fake.calls[0][2]["image_url"] == "https://x/i.jpg"


def test_upload_pin(client):
    c, fake = client
    r = c.post(
        "/v1/pins/upload",
        files={"file": ("a.jpg", b"\xff\xd8\xff", "image/jpeg")},
        data={"board_id": "b1", "title": "t"},
    )
    assert r.status_code == 200
    assert fake.calls[0][0] == "upload_pin"
    assert fake.calls[0][2]["board_id"] == "b1"


def test_delete_pin(client):
    c, fake = client
    assert c.delete("/v1/pins/42").status_code == 200
    assert fake.calls[0][0] == "delete_pin"


def test_board_endpoints(client):
    c, fake = client
    assert c.post("/v1/boards", json={"name": "B"}).status_code == 200
    assert fake.calls[0][0] == "create_board"
    assert c.get("/v1/boards/bid/pins", params={"pages": 3}).json()[2]["page"] == 2
    assert c.post("/v1/boards/bid/sections", json={"section_name": "s"}).status_code == 200
    assert c.get("/v1/sections/sid/pins").status_code == 200
    assert c.post("/v1/boards/bid/follow").status_code == 200
    assert c.post("/v1/boards/bid/unfollow").status_code == 200


def test_user_endpoints(client):
    c, _ = client
    for path in [
        "/v1/users/u/overview",
        "/v1/users/u/pins",
        "/v1/users/u/boards",
        "/v1/users/u/followers",
        "/v1/users/u/following",
    ]:
        assert c.get(path).status_code == 200, path
    assert c.post("/v1/users/uid/follow").status_code == 200
    assert c.post("/v1/users/uid/unfollow").status_code == 200


def test_comments_and_messages(client):
    c, fake = client
    assert c.get("/v1/pins/p/comments").status_code == 200
    assert c.post("/v1/pins/p/comments", json={"text": "hey"}).status_code == 200
    assert c.delete("/v1/pins/p/comments/cid").status_code == 200
    assert c.get("/v1/conversations").status_code == 200
    assert c.post("/v1/conversations", json={"user_ids": ["u1"]}).status_code == 200
    assert c.post(
        "/v1/conversations/c1/messages", json={"message": "hi", "pin_id": "p9"}
    ).status_code == 200


def test_auth_required(client, monkeypatch):
    monkeypatch.setenv("PINAPI_API_KEYS", "k1,k2")
    from pinapi.config import get_settings

    get_settings.cache_clear()
    c, _ = client
    try:
        assert c.get("/v1/status").status_code == 401
        assert c.get("/v1/status", headers={"Authorization": "Bearer bad"}).status_code == 401
        assert (
            c.get("/v1/status", headers={"Authorization": "Bearer k1"}).status_code == 200
        )
    finally:
        monkeypatch.delenv("PINAPI_API_KEYS")
        get_settings.cache_clear()
