from functools import lru_cache

from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    pinterest_email: str = ""
    pinterest_password: str = ""
    pinterest_username: str = ""
    pinterest_proxy: str = ""

    # comma-separated API keys; empty means the API is open (self-hosted dev mode)
    pinapi_api_keys: str = ""

    pinapi_cred_root: str = "./data/creds"
    pinapi_auto_login: bool = True
    pinapi_login_lang: str = "en"
    pinapi_max_pages: int = 10
    pinapi_upload_dir: str = "./data/uploads"

    @property
    def api_keys(self) -> set[str]:
        return {k.strip() for k in self.pinapi_api_keys.split(",") if k.strip()}

    @property
    def proxies(self) -> dict | None:
        if not self.pinterest_proxy:
            return None
        return {"http": self.pinterest_proxy, "https": self.pinterest_proxy}


@lru_cache
def get_settings() -> Settings:
    return Settings()
