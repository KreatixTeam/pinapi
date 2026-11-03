"""Thread-safe wrapper around py3-pinterest with lazy login and session recovery."""

from __future__ import annotations

import logging
import os
import threading
from typing import Any

import requests
from py3pin.Pinterest import Pinterest

from .config import Settings

log = logging.getLogger("pinapi.client")

_chrome_patched = False


def _apply_chrome_patches() -> None:
    """Adapt py3-pinterest's selenium login for containerized Chrome.

    PINAPI_CHROME_NO_SANDBOX=1 adds --no-sandbox/--disable-dev-shm-usage (needed
    when running as root in Docker). PINAPI_CHROME_BINARY and
    PINAPI_CHROMEDRIVER_PATH override the binary/driver locations.
    """
    global _chrome_patched
    if _chrome_patched:
        return
    import py3pin.Pinterest as pmod

    orig_options = pmod.ChromeOptions

    def options_factory(*args, **kwargs):
        opts = orig_options(*args, **kwargs)
        if os.environ.get("PINAPI_CHROME_NO_SANDBOX"):
            opts.add_argument("--no-sandbox")
            opts.add_argument("--disable-dev-shm-usage")
        binary = os.environ.get("PINAPI_CHROME_BINARY")
        if binary:
            opts.binary_location = binary
        return opts

    orig_cdm = pmod.ChromeDriverManager

    class CDM(orig_cdm):
        def install(self):
            path = os.environ.get("PINAPI_CHROMEDRIVER_PATH")
            return path or super().install()

    pmod.ChromeOptions = options_factory
    pmod.ChromeDriverManager = CDM
    _chrome_patched = True


class PinterestError(Exception):
    """Pinterest-side failure, carries an HTTP-ish status for the API layer."""

    def __init__(self, detail: str, status_code: int = 502):
        super().__init__(detail)
        self.detail = detail
        self.status_code = status_code


class PinterestClient:
    def __init__(self, settings: Settings):
        self.settings = settings
        self._lock = threading.Lock()
        self._pinterest: Pinterest | None = None

    def _get(self) -> Pinterest:
        if self._pinterest is None:
            self._pinterest = Pinterest(
                email=self.settings.pinterest_email,
                password=self.settings.pinterest_password,
                username=self.settings.pinterest_username,
                cred_root=self.settings.pinapi_cred_root,
                proxies=self.settings.proxies,
            )
        return self._pinterest

    @property
    def has_session(self) -> bool:
        p = self._get()
        return bool(p.http.cookies.get("_pinterest_sess") or p.http.cookies.get("csrftoken"))

    def _login(self) -> None:
        if not (self.settings.pinterest_email and self.settings.pinterest_password):
            raise PinterestError(
                "no pinterest session and PINTEREST_EMAIL/PINTEREST_PASSWORD are not set",
                status_code=401,
            )
        log.info("logging in to pinterest as %s", self.settings.pinterest_email)
        _apply_chrome_patches()
        self._get().login(lang=self.settings.pinapi_login_lang)
        if not self.has_session:
            raise PinterestError("pinterest login failed", status_code=401)

    def call(self, method: str, *args: Any, **kwargs: Any) -> Any:
        """Call a py3-pinterest method serially; relogin once on 401/403."""
        with self._lock:
            fn = getattr(self._get(), method)
            try:
                result = fn(*args, **kwargs)
            except requests.HTTPError as e:
                status = e.response.status_code if e.response is not None else 502
                if status in (401, 403) and self.settings.pinapi_auto_login:
                    self._login()
                    result = fn(*args, **kwargs)
                else:
                    raise PinterestError(str(e), status_code=status) from e
            except PinterestError:
                raise
            except Exception as e:
                raise PinterestError(str(e)) from e
            return self._unwrap(result)

    def call_pages(self, method: str, pages: int, *args: Any, **kwargs: Any) -> list:
        """Fetch multiple batches of a paginated method. First call resets the bookmark."""
        items: list = []
        for i in range(pages):
            batch = self.call(method, *args, reset_bookmark=(i == 0), **kwargs)
            if not batch:
                break
            items.extend(batch if isinstance(batch, list) else [batch])
        return items

    @staticmethod
    def _unwrap(result: Any) -> Any:
        # write methods return requests.Response; readers return parsed data already
        if isinstance(result, requests.Response):
            try:
                return result.json()
            except ValueError:
                return {"status_code": result.status_code}
        return result


def get_client(settings: Settings) -> PinterestClient:
    return PinterestClient(settings)
