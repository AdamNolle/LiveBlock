#!/usr/bin/env python3
"""Fetch a provenance-preserving, commercially usable sports-ad image pool.

Images are discovered through Wikimedia Commons but every file's own metadata
is checked before download. By default only Public Domain, CC0, and CC BY files
are accepted. CC BY-SA is intentionally excluded unless --allow-share-alike is
explicitly supplied.

This creates an *annotation pool*, not training labels. Images must be reviewed
and annotated before promotion into train/val/test.
"""
from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

API = "https://commons.wikimedia.org/w/api.php"
USER_AGENT = "LiveBlockCorpusBuilder/0.1 (local research; contact via repository)"
BASE_ALLOWED = {"cc0", "public domain", "pd", "cc by 2.0", "cc by 3.0", "cc by 4.0"}
SHARE_ALIKE = {"cc by-sa 2.0", "cc by-sa 3.0", "cc by-sa 4.0"}
SUPPORTED_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}


def clean_html(value: str | None) -> str:
    if not value:
        return ""
    return re.sub(r"\s+", " ", re.sub(r"<[^>]+>", "", html.unescape(value))).strip()


def open_with_backoff(request: urllib.request.Request, timeout: int):
    for attempt in range(6):
        try:
            return urllib.request.urlopen(request, timeout=timeout)
        except urllib.error.HTTPError as error:
            if error.code != 429 or attempt == 5:
                raise
            retry_after = error.headers.get("Retry-After")
            delay = float(retry_after) if retry_after and retry_after.isdigit() else 2 ** attempt
            time.sleep(min(60, max(1, delay)))
    raise RuntimeError("unreachable")


def api_json(params: dict[str, str | int]) -> dict:
    query = urllib.parse.urlencode({"format": "json", "formatversion": 2, **params})
    request = urllib.request.Request(f"{API}?{query}", headers={"User-Agent": USER_AGENT})
    with open_with_backoff(request, timeout=60) as response:
        return json.load(response)


def download(url: str, destination: Path) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    digest = hashlib.sha256()
    with open_with_backoff(request, timeout=120) as response, destination.open("wb") as output:
        while chunk := response.read(1024 * 1024):
            digest.update(chunk)
            output.write(chunk)
    return digest.hexdigest()


def metadata_value(metadata: dict, key: str) -> str:
    value = metadata.get(key, {})
    return clean_html(value.get("value") if isinstance(value, dict) else str(value))


def normalized_license(metadata: dict) -> str:
    return metadata_value(metadata, "LicenseShortName").casefold().replace("creative commons ", "cc ")


def fetch_source(context: str, output: Path, limit: int, allow_share_alike: bool,
                 *, category: bool = False, request_delay: float = 0.5,
                 shared_split_group: str | None = None,
                 title_terms: tuple[str, ...] = ()) -> int:
    """Fetch either full-text search results or files from a Commons category."""
    output.mkdir(parents=True, exist_ok=True)
    manifest = output / "manifest.jsonl"
    allowed = BASE_ALLOWED | (SHARE_ALIKE if allow_share_alike else set())
    accepted = len([line for line in manifest.read_text().splitlines() if line.strip()]) if manifest.exists() else 0
    continuation: str | int | None = None

    while accepted < limit:
        request_params: dict[str, str | int] = {
            "action": "query",
            "prop": "imageinfo",
            "iiprop": "url|extmetadata|mime|size",
            "iiurlwidth": 1920,
        }
        if category:
            request_params.update({
                "generator": "categorymembers",
                "gcmtitle": context if context.startswith("Category:") else f"Category:{context}",
                "gcmtype": "file",
                "gcmlimit": min(50, max(10, limit - accepted)),
            })
            if continuation is not None:
                request_params["gcmcontinue"] = continuation
        else:
            request_params.update({
                "generator": "search",
                "gsrsearch": context,
                "gsrnamespace": 6,
                "gsrlimit": min(50, max(10, limit - accepted)),
                "gsroffset": int(continuation or 0),
            })

        payload = api_json(request_params)
        pages = payload.get("query", {}).get("pages", [])
        if not pages:
            break
        if category:
            continuation = payload.get("continue", {}).get("gcmcontinue")
        else:
            continuation = int(continuation or 0) + len(pages)

        for page in pages:
            if accepted >= limit:
                break
            title = str(page.get("title", ""))
            if title_terms and not any(term.casefold() in title.casefold() for term in title_terms):
                continue
            info = (page.get("imageinfo") or [{}])[0]
            metadata = info.get("extmetadata") or {}
            license_name = normalized_license(metadata)
            if license_name not in allowed:
                continue
            source_url = info.get("descriptionurl") or page.get("canonicalurl")
            image_url = info.get("thumburl") or info.get("url")
            if not source_url or not image_url:
                continue
            suffix = Path(urllib.parse.urlparse(info.get("url", image_url)).path).suffix.lower()
            if suffix not in SUPPORTED_EXTENSIONS:
                continue

            stable_id = hashlib.sha256(source_url.encode()).hexdigest()[:20]
            destination = output / f"{stable_id}{suffix}"
            if destination.exists():
                continue
            try:
                sha256 = download(image_url, destination)
            except Exception as error:
                print(f"warning: download failed for {source_url}: {error}")
                destination.unlink(missing_ok=True)
                continue

            record = {
                "annotation_status": "pending",
                "artist": metadata_value(metadata, "Artist"),
                "attribution": metadata_value(metadata, "Attribution"),
                "categories": ["sports_ad_candidate"],
                "depicted_context": context,
                "height": info.get("thumbheight") or info.get("height"),
                "image_url": image_url,
                "license": metadata_value(metadata, "LicenseShortName"),
                "license_url": metadata_value(metadata, "LicenseUrl"),
                "local_path": destination.name,
                "retrieved_at": datetime.now(timezone.utc).isoformat(),
                "sha256": sha256,
                "source": "wikimedia_commons",
                "source_page": source_url,
                "split_group": shared_split_group or source_url,
                "title": title,
                "width": info.get("thumbwidth") or info.get("width"),
            }
            with manifest.open("a", encoding="utf-8") as handle:
                handle.write(json.dumps(record, sort_keys=True) + "\n")
            accepted += 1
            print(f"[{accepted}/{limit}] {destination.name} · {record['license']} · {record['title']}")
            time.sleep(max(0, request_delay))

        if category and continuation is None:
            break

    return accepted


def fetch_query(query: str, output: Path, limit: int, allow_share_alike: bool,
                request_delay: float = 0.5,
                title_terms: tuple[str, ...] = ()) -> int:
    return fetch_source(query, output, limit, allow_share_alike,
                        request_delay=request_delay, title_terms=title_terms)


def fetch_category(category: str, output: Path, limit: int, allow_share_alike: bool,
                   request_delay: float = 0.5, group_members: bool = False,
                   title_terms: tuple[str, ...] = ()) -> int:
    shared_group = f"wikimedia-category:{category}" if group_members else None
    return fetch_source(category, output, limit, allow_share_alike,
                        category=True, request_delay=request_delay,
                        shared_split_group=shared_group, title_terms=title_terms)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--query", action="append", default=[],
                        help="Commons full-text query; repeat for multiple contexts")
    parser.add_argument("--category", action="append", default=[],
                        help="Commons category name; repeat for multiple categories")
    parser.add_argument("--limit-per-query", type=int, default=25)
    parser.add_argument("--request-delay", type=float, default=0.5,
                        help="seconds between accepted downloads; increase when Commons throttles")
    parser.add_argument("--group-category-members", action="store_true",
                        help="assign every file in each category one split group (use for event/frame categories)")
    parser.add_argument("--require-title-term", action="append", default=[],
                        help="download only results whose Commons title contains any supplied term; repeatable")
    parser.add_argument("--allow-share-alike", action="store_true")
    args = parser.parse_args()

    if not args.query and not args.category:
        parser.error("at least one --query or --category is required")

    if args.request_delay < 0:
        parser.error("--request-delay must be non-negative")

    total = 0
    for query in args.query:
        target = args.output / re.sub(r"[^a-z0-9]+", "-", query.casefold()).strip("-")
        total += fetch_query(query, target, args.limit_per_query,
                             args.allow_share_alike, args.request_delay,
                             tuple(args.require_title_term))
    for category in args.category:
        target = args.output / re.sub(r"[^a-z0-9]+", "-", category.casefold()).strip("-")
        total += fetch_category(category, target, args.limit_per_query,
                                args.allow_share_alike, args.request_delay,
                                args.group_category_members,
                                tuple(args.require_title_term))
    print(f"fetched {total} provenance-checked images into {args.output}")
    return 0 if total else 2


if __name__ == "__main__":
    raise SystemExit(main())
