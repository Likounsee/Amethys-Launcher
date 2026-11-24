#!/usr/bin/env python3
"""Build the public, compressed Amethys PC master catalogue from IGDB.

Secrets are read only from the process environment and are never written to the
database, logs, generated files, or GitHub Release assets.
"""
from __future__ import annotations

import datetime as dt
import json
import os
import sqlite3
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import zstandard

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "scripts" / "catalog_schema.sql"
MASTER_JSON = ROOT / "catalogs" / "master-games-v1.json"
REGISTRY_PATH = ROOT / "catalogs" / "amethys-id-registry.json"
DB_PATH = ROOT / "catalog.db"
COMPRESSED_PATH = ROOT / "catalog.db.zst"
TOKEN_URL = "https://id.twitch.tv/oauth2/token"
API_ROOT = "https://api.igdb.com/v4"
PAGE_SIZE = 500
MAX_RETRIES = 6
WINDOWS_PC_PLATFORM_ID = 6

# Only stores that Amethys currently exposes as supported platform identities.
# itch.io is intentionally excluded. Unrecognized IGDB sources are ignored.
STORE_NAMES = {
    "steam": "steam",
    "gog": "gog",
    "epic games store": "epic",
    "epic game store": "epic",
    "epic games": "epic",
    "ea app": "ea",
    "origin": "ea",
    "ubisoft connect": "ubisoft",
    "ubisoft store": "ubisoft",
    "battle.net": "battle-net",
    "blizzard shop": "battle-net",
    "amazon games": "amazon-games",
    "humble store": "humble",
}


def http_request(url: str, *, data: bytes | None = None,
                 headers: dict[str, str] | None = None) -> bytes:
    request = urllib.request.Request(url, data=data, headers=headers or {})
    for attempt in range(MAX_RETRIES):
        try:
            with urllib.request.urlopen(request, timeout=90) as response:
                return response.read()
        except urllib.error.HTTPError as error:
            if error.code not in (429, 500, 502, 503, 504) or attempt == MAX_RETRIES - 1:
                detail = error.read().decode("utf-8", "replace")
                raise RuntimeError(f"IGDB HTTP {error.code}: {detail[:500]}") from error
            retry_after = error.headers.get("Retry-After")
            time.sleep(float(retry_after) if retry_after else min(2 ** attempt, 30))
        except (urllib.error.URLError, TimeoutError):
            if attempt == MAX_RETRIES - 1:
                raise
            time.sleep(min(2 ** attempt, 30))
    raise RuntimeError("IGDB request retry limit reached")


def get_token(client_id: str, client_secret: str) -> str:
    query = urllib.parse.urlencode({
        "client_id": client_id,
        "client_secret": client_secret,
        "grant_type": "client_credentials",
    })
    payload = json.loads(http_request(
        f"{TOKEN_URL}?{query}", data=b"",
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    ))
    token = payload.get("access_token")
    if not token:
        raise RuntimeError("Twitch OAuth response has no access_token")
    return token


def igdb_query(body: str, client_id: str, token: str) -> list[dict]:
    payload = http_request(f"{API_ROOT}/games", data=body.encode("utf-8"), headers={
        "Client-ID": client_id,
        "Authorization": f"Bearer {token}",
        "Accept": "application/json",
        "Content-Type": "text/plain",
    })
    decoded = json.loads(payload)
    if not isinstance(decoded, list):
        raise RuntimeError("Unexpected IGDB response (expected an array)")
    return decoded


def load_identity_state() -> tuple[dict[int, int], set[int], dict[tuple[str, str], int]]:
    registry = {"igdb_to_amethys_id": {}, "retired_or_known_ids": []}
    if REGISTRY_PATH.exists():
        registry = json.loads(REGISTRY_PATH.read_text(encoding="utf-8"))
    igdb_to_amethys = {
        int(key): int(value)
        for key, value in registry.get("igdb_to_amethys_id", {}).items()
    }
    known_ids = {int(value) for value in registry.get("retired_or_known_ids", [])}
    previous_external: dict[tuple[str, str], int] = {}

    # Preserve the IDs already shipped in the small seed catalogue on first run.
    if MASTER_JSON.exists():
        previous = json.loads(MASTER_JSON.read_text(encoding="utf-8"))
        for game in previous.get("games", []):
            amethys_id = int(game["amethys_id"])
            known_ids.add(amethys_id)
            raw_igdb_id = game.get("igdb_id")
            if raw_igdb_id is None and str(game.get("id", "")).startswith("igdb-"):
                try:
                    raw_igdb_id = int(game["id"][5:])
                except ValueError:
                    raw_igdb_id = None
            if raw_igdb_id is not None:
                igdb_to_amethys.setdefault(int(raw_igdb_id), amethys_id)
            for offer in game.get("offers", []):
                provider = str(offer.get("provider_id", "")).lower()
                external_id = str(offer.get("external_id", "")).strip()
                if provider in ("steam", "gog", "epic", "epic-games") and external_id:
                    store = "epic" if provider == "epic-games" else provider
                    previous_external[(store, external_id)] = amethys_id

    known_ids.update(igdb_to_amethys.values())
    return igdb_to_amethys, known_ids, previous_external


def store_record(game: dict, external: dict) -> tuple[str, str, str, str] | None:
    source = external.get("external_game_source") or {}
    source_name = str(source.get("name", "")).strip().lower()
    store = STORE_NAMES.get(source_name)
    if not store:  # includes itch.io and sources we do not support
        return None
    external_id = str(external.get("uid") or "").strip()
    if not external_id:
        return None
    url = external.get("url")
    if not isinstance(url, str) or not url.startswith("https://"):
        if store == "steam" and external_id.isdigit():
            url = f"https://store.steampowered.com/app/{external_id}/"
        else:
            return None
    name = str(external.get("name") or game.get("name") or "").strip()
    return store, external_id, name, url


def main() -> None:
    client_id = os.environ.get("IGDB_CLIENT_ID", "").strip()
    client_secret = os.environ.get("IGDB_CLIENT_SECRET", "").strip()
    if not client_id or not client_secret:
        raise SystemExit("Missing IGDB_CLIENT_ID / IGDB_CLIENT_SECRET environment secrets")

    igdb_to_amethys, known_ids, previous_external = load_identity_state()
    next_amethys_id = max(known_ids | {1_000_000}) + 1
    token = get_token(client_id, client_secret)
    generated_at = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()

    DB_PATH.unlink(missing_ok=True)
    connection = sqlite3.connect(DB_PATH)
    connection.execute("PRAGMA journal_mode = OFF")
    connection.execute("PRAGMA synchronous = OFF")
    connection.execute("PRAGMA temp_store = MEMORY")
    connection.executescript(SCHEMA_PATH.read_text(encoding="utf-8"))

    inserted_games = 0
    inserted_stores = 0
    skipped_duplicate_store_ids = 0
    last_igdb_id = 0
    fields = (
        "id,name,slug,summary,first_release_date,cover.image_id,genres.name,"
        "external_games.uid,external_games.name,external_games.url,"
        "external_games.external_game_source.name"
    )

    try:
        while True:
            # Keyset pagination avoids IGDB offset ceilings and remains stable
            # when the catalogue is larger than 10,000 records.
            query = (
                f"fields {fields}; "
                f"where category = 0 & platforms = ({WINDOWS_PC_PLATFORM_ID}) "
                f"& id > {last_igdb_id}; "
                f"sort id asc; limit {PAGE_SIZE};"
            )
            page = igdb_query(query, client_id, token)
            if not page:
                break

            with connection:
                for game in page:
                    igdb_id = int(game["id"])
                    last_igdb_id = max(last_igdb_id, igdb_id)
                    name = str(game.get("name") or "").strip()
                    if not name:
                        continue

                    store_rows = []
                    for external in game.get("external_games") or []:
                        record = store_record(game, external)
                        if record:
                            store_rows.append(record)

                    amethys_id = igdb_to_amethys.get(igdb_id)
                    if amethys_id is None:
                        for store, external_id, _store_name, _url in store_rows:
                            amethys_id = previous_external.get((store, external_id))
                            if amethys_id is not None:
                                break
                    if amethys_id is None:
                        while next_amethys_id in known_ids:
                            next_amethys_id += 1
                        amethys_id = next_amethys_id
                        next_amethys_id += 1
                    if amethys_id in known_ids and igdb_id not in igdb_to_amethys:
                        # A legacy ID can be reused only when matched by an
                        # existing store identity; otherwise allocate a fresh ID.
                        matched = any(
                            previous_external.get((store, external_id)) == amethys_id
                            for store, external_id, _n, _u in store_rows
                        )
                        if not matched:
                            while next_amethys_id in known_ids:
                                next_amethys_id += 1
                            amethys_id = next_amethys_id
                            next_amethys_id += 1

                    igdb_to_amethys[igdb_id] = amethys_id
                    known_ids.add(amethys_id)
                    release_timestamp = game.get("first_release_date")
                    release_date = (
                        dt.datetime.fromtimestamp(release_timestamp, tz=dt.timezone.utc)
                        .date().isoformat()
                        if isinstance(release_timestamp, (int, float)) else None
                    )
                    cover_id = (game.get("cover") or {}).get("image_id")
                    cover_url = (
                        f"https://images.igdb.com/igdb/image/upload/t_cover_big/{cover_id}.jpg"
                        if cover_id else None
                    )
                    genres = sorted({
                        str(item.get("name", "")).strip()
                        for item in game.get("genres", []) if item.get("name")
                    })
                    connection.execute(
                        """INSERT INTO games
                           (igdb_id, amethys_id, name, slug, summary, release_date,
                            cover_url, genres_json, updated_at)
                           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                        (igdb_id, amethys_id, name, game.get("slug"), game.get("summary"),
                         release_date, cover_url, json.dumps(genres, ensure_ascii=False),
                         generated_at),
                    )
                    inserted_games += 1

                    for store, external_id, store_name, store_url in store_rows:
                        try:
                            connection.execute(
                                """INSERT INTO game_stores
                                   (igdb_game_id, store, external_id, store_name, store_url)
                                   VALUES (?, ?, ?, ?, ?)""",
                                (igdb_id, store, external_id, store_name, store_url),
                            )
                            inserted_stores += 1
                        except sqlite3.IntegrityError:
                            # IGDB can expose one external product against more
                            # than one record; never create an ambiguous mapping.
                            skipped_duplicate_store_ids += 1

            print(
                f"IGDB cursor={last_igdb_id}; games={inserted_games}; "
                f"store IDs={inserted_stores}",
                file=sys.stderr,
            )
            if len(page) < PAGE_SIZE:
                break
            time.sleep(0.30)  # IGDB limit: 4 requests/second.

        if inserted_games == 0:
            raise RuntimeError("IGDB returned zero Windows PC games; refusing to publish an empty catalogue")

        connection.executemany(
            "INSERT INTO catalog_meta(key, value) VALUES (?, ?)",
            [
                ("schema_version", "1"),
                ("source", "IGDB"),
                ("platform_filter", "Windows PC (IGDB platform 6)"),
                ("category_filter", "0"),
                ("generated_at", generated_at),
                ("game_count", str(inserted_games)),
                ("store_identity_count", str(inserted_stores)),
            ],
        )
        connection.execute("ANALYZE")
        connection.execute("PRAGMA optimize")
        integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
        if integrity != "ok":
            raise RuntimeError(f"SQLite integrity_check failed: {integrity}")
    finally:
        connection.close()

    REGISTRY_PATH.parent.mkdir(parents=True, exist_ok=True)
    REGISTRY_PATH.write_text(json.dumps({
        "schema_version": 1,
        "igdb_to_amethys_id": {
            str(key): value for key, value in sorted(igdb_to_amethys.items())
        },
        "retired_or_known_ids": sorted(known_ids),
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    temporary_output = COMPRESSED_PATH.with_suffix(".zst.tmp")
    with DB_PATH.open("rb") as source, temporary_output.open("wb") as target:
        zstandard.ZstdCompressor(level=19, threads=-1).copy_stream(source, target)
    temporary_output.replace(COMPRESSED_PATH)

    print(
        f"Built {inserted_games} Windows PC games, {inserted_stores} store identities; "
        f"skipped {skipped_duplicate_store_ids} duplicate store mappings. "
        f"SQLite={DB_PATH.stat().st_size:,} bytes; "
        f"Zstandard={COMPRESSED_PATH.stat().st_size:,} bytes."
    )


if __name__ == "__main__":
    main()
