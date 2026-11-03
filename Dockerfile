FROM python:3.12-slim

# Chrome is required for the (rare) interactive login; cookies persist for ~15 days.
RUN apt-get update \
    && apt-get install -y --no-install-recommends wget ca-certificates ffmpeg \
    && wget -qO /tmp/chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb \
    && apt-get install -y --no-install-recommends /tmp/chrome.deb \
    && rm /tmp/chrome.deb \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY pinapi ./pinapi

ENV PINAPI_CHROME_NO_SANDBOX=1 \
    PINAPI_CRED_ROOT=/data/creds \
    PINAPI_UPLOAD_DIR=/data/uploads
VOLUME /data
EXPOSE 8000

CMD ["uvicorn", "pinapi.main:app", "--host", "0.0.0.0", "--port", "8000"]
