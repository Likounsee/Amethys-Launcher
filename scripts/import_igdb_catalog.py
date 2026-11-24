#!/usr/bin/env python3
"""Build Amethys' shared game catalogue from IGDB.

Required environment:
  IGDB_CLIENT_ID
  IGDB_CLIENT_SECRET

The script never invents Amethys IDs from local installs. It preserves IDs from
the current master file (keyed by IGDB game ID) and allocates new IDs centrally
in the generated repository catalogue.
"""
from __future__ import annotations

import datetime as dt
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = ROOT / "catalogs" / "master-games-v1.json"
REGISTRY_PATH = ROOT / "catalogs" / "amethys-id-registry.json"
TOKEN_URL = "https://id.twitch.tv/oauth2/token"
API_ROOT = "https://api.igdb.com/v4"
PAGE_SIZE = 500
MAX_RETRIES = 6
# IGDB platform IDs: Linux=3, Windows=6, Mac=14.
PC_PLATFORM_IDS = (3, 6, 14)
SOURCE_TO_PROVIDER = {
    "steam": "steam",
    "epic games store": "epic-games",
    "epic game store": "epic-games",
    "gog": "gog",
}


def request(url: str, *, data: bytes | None = None, headers: dict[str, str] | None = None):
    req = urllib.request.Request(url, data=data, headers=headers or {})
    for attempt in range(MAX_RETRIES):
        try:
            with urllib.request.urlopen(req, timeout=60) as response:
                return response.read()
        except urllib.error.HTTPError as error:
            if error.code not in (429, 500, 502, 503, 504) or attempt == MAX_RETRIES - 1:
                detail = error.read().decode("utf-8", "replace")
                raise RuntimeError(f"HTTP {error.code} for {url}: {detail[:1000]}") from error
            retry_after = error.headers.get("Retry-After")
            time.sleep(float(retry_after) if retry_after else min(2 ** attempt, 30))
        except (urllib.error.URLError, TimeoutError):
            if attempt == MAX_RETRIES - 1:
                raise
            time.sleep(min(2 ** attempt, 30))
    raise RuntimeError("request retry loop exhausted")


def get_token(client_id: str, client_secret: str) -> str:
    query = urllib.parse.urlencode({
        "client_id": client_id,
        "client_secret": client_secret,
        "grant_type": "client_credentials",
    })
    payload = json.loads(request(f"{TOKEN_URL}?{query}", data=b"", headers={
        "Content-Type": "application/x-www-form-urlencoded",
    }))
    token = payload.get("access_token")
    if not token:
        raise RuntimeError("Twitch OAuth response did not contain access_token")
    return token


def igdb_query(endpoint: str, body: str, client_id: str, token: str):
    raw = request(f"{API_ROOT}/{endpoint}", data=body.encode("utf-8"), headers={
        "Client-ID": client_id,
        "Authorization": f"Bearer {token}",
        "Accept": "application/json",
        "Content-Type": "text/plain",
    })
    return json.loads(raw)


def load_existing():
    if not CATALOG_PATH.exists():
        return {"schema_version": 1, "catalog_id": "amethys.master-games",
                "catalog_version": 0, "games": []}
    return json.loads(CATALOG_PATH.read_text(encoding="utf-8"))


def make_offer(game_id: int, external: dict, index: int):
    source = external.get("external_game_source") or {}
    source_name = str(source.get("name", "")).strip().lower()
    provider_id = SOURCE_TO_PROVIDER.get(source_name)
    uid = external.get("uid")
    url = external.get("url")
    if not provider_id or uid is None:
        return None
    uid = str(uid).strip()
    if not uid:
        return None
    if not isinstance(url, str) or not url.startswith("https://"):
        if provider_id == "steam" and uid.isdigit():
            url = f"https://store.steampowered.com/app/{uid}/"
        else:
            return None
    return {
        "id": f"igdb-{game_id}-{provider_id}-{index}",
        "provider_id": provider_id,
        "external_id": uid,
        "title": external.get("name"),
        "distribution": "external_link",
        "capabilities": {
            "official_launch": False,
            "direct_install": False,
            "update": False,
            "repair": False,
        },
        "external_url": url,
    }


def main():
    client_id = os.environ.get("IGDB_CLIENT_ID", "").strip()
    client_secret = os.environ.get("IGDB_CLIENT_SECRET", "").strip()
    if not client_id or not client_secret:
        raise SystemExit(
            "Set IGDB_CLIENT_ID and IGDB_CLIENT_SECRET (Twitch Developer application credentials)."
        )

    old = load_existing()
    registry = {"schema_version": 1, "igdb_to_amethys_id": {}, "retired_or_known_ids": []}
    if REGISTRY_PATH.exists():
        registry = json.loads(REGISTRY_PATH.read_text(encoding="utf-8"))
    igdb_to_amethys = {
        int(key): int(value)
        for key, value in registry.get("igdb_to_amethys_id", {}).items()
    }
    used_ids = {int(value) for value in registry.get("retired_or_known_ids", [])}
    previous_by_external = {}
    for game in old.get("games", []):
        igdb_id = game.get("igdb_id")
        if igdb_id is None and str(game.get("id", "")).startswith("igdb-"):
            try:
                igdb_id = int(game["id"][5:])
            except ValueError:
                pass
        if igdb_id is not None:
            igdb_to_amethys.setdefault(int(igdb_id), int(game["amethys_id"]))
        used_ids.add(int(game["amethys_id"]))
        for offer in game.get("offers", []):
            previous_by_external[
                (offer.get("provider_id"), str(offer.get("external_id", "")))
            ] = int(game["amethys_id"])

    used_ids.update(igdb_to_amethys.values())
    next_id = max(used_ids | {1_000_000}) + 1
    token = get_token(client_id, client_secret)
    games = []
    offset = 0
    platform_filter = ",".join(str(value) for value in PC_PLATFORM_IDS)
    fields = (
        "id,name,slug,summary,first_release_date,cover.image_id,genres.name,"
        "platforms.name,game_type.name,"
        "external_games.uid,external_games.name,external_games.url,"
        "external_games.external_game_source.name"
    )

    while True:
        body = (
            f"fields {fields}; where platforms = ({platform_filter}); "
            f"sort id asc; limit {PAGE_SIZE}; offset {offset};"
        )
        page = igdb_query("games", body, client_id, token)
        if not page:
            break
        for item in page:
            igdb_id = int(item["id"])
            name = str(item.get("name", "")).strip()
            if not name:
                continue
            genres = sorted({
                str(genre.get("name", "")).strip()
                for genre in item.get("genres", [])
                if genre.get("name")
            })
            offers = []
            for index, external in enumerate(item.get("external_games", [])):
                offer = make_offer(igdb_id, external, index)
                if offer:
                    offers.append(offer)
            known_id = igdb_to_amethys.get(igdb_id)
            if known_id is None:
                for offer in offers:
                    known_id = previous_by_external.get(
                        (offer["provider_id"], offer["external_id"])
                    )
                    if known_id is not None:
                        break
            if known_id is None:
                known_id = next_id
                next_id += 1
            igdb_to_amethys[igdb_id] = known_id
            used_ids.add(known_id)
            amethys_id = known_id
            release_date = item.get("first_release_date")
            release = None
            if isinstance(release_date, (int, float)):
                release = dt.datetime.fromtimestamp(
                    release_date, tz=dt.timezone.utc
                ).date().isoformat()
            cover = (item.get("cover") or {}).get("image_id")
            game = {
                "amethys_id": amethys_id,
                "id": f"igdb-{igdb_id}",
                "title": name,
                "description": item.get("summary"),
                "genres": genres,
                "developer": None,
                "publisher": None,
                "release_date": release,
                "cover_ref": (
                    f"https://images.igdb.com/igdb/image/upload/t_cover_big/{cover}.jpg"
                    if cover else None
                ),
                "banner_ref": None,
                "offers": offers,
            }
            games.append(game)
        print(f"Fetched {len(games)} PC games (offset {offset})", file=sys.stderr)
        if len(page) < PAGE_SIZE:
            break
        offset += PAGE_SIZE
        # IGDB documents a four-request-per-second rate limit.
        time.sleep(0.30)

    games.sort(key=lambda game: game["amethys_id"])
    ids = [game["amethys_id"] for game in games]
    if len(ids) != len(set(ids)):
        raise RuntimeError("duplicate Amethys IDs generated")
    external_keys = [
        (offer["provider_id"], offer["external_id"])
        for game in games for offer in game["offers"]
    ]
    if len(external_keys) != len(set(external_keys)):
        raise RuntimeError("duplicate platform/external game identity in IGDB data")

    document = {
        "schema_version": 1,
        "catalog_id": "amethys.master-games",
        "catalog_version": int(old.get("catalog_version", 0)) + 1,
        "generated_at": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
        "games": games,
    }
    REGISTRY_PATH.write_text(
        json.dumps({
            "schema_version": 1,
            "igdb_to_amethys_id": {
                str(key): value for key, value in sorted(igdb_to_amethys.items())
            },
            "retired_or_known_ids": sorted(used_ids),
        }, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    CATALOG_PATH.write_text(
        json.dumps(document, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(
        f"Wrote {len(games)} PC games and "
        f"{sum(len(game['offers']) for game in games)} verified IGDB store identities "
        f"to {CATALOG_PATH}"
    )


if __name__ == "__main__":
    main()
