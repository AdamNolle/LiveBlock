#!/usr/bin/env python3
"""Local browser UI for human review of corpus labels.

Runs only on loopback. Reviewers can draw/delete sponsor boxes and team-identity
preservation regions, then approve an image; approval atomically writes
`review_method: human` to the sibling label file.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import secrets
import tempfile
import threading
import urllib.parse
import webbrowser
from collections import defaultdict
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from corpus.build_sports_corpus import (HUMAN_REVIEW_ATTESTATION, PLACEMENT_KINDS,
                                        REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
                                        REQUIRED_PROMOTION_PLACEMENTS,
                                        REQUIRED_PROMOTION_PRESERVATION_KINDS,
                                        validate_box, validate_preserve_region)

IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}
HTML = Path(__file__).with_name("review_ui.html").read_text()
MAX_REQUEST_BYTES = 1_048_576


def validate_reviewer_identity(value: str) -> str:
    reviewer = str(value).strip()
    if not reviewer:
        raise ValueError("reviewer identity is required")
    if len(reviewer) > 200 or any(ord(character) < 32 for character in reviewer):
        raise ValueError("reviewer identity must be at most 200 printable characters")
    return reviewer


class ReviewSession:
    """One immutable attributable reviewer identity per server process."""

    def __init__(self, reviewer: str | None = None):
        self._lock = threading.Lock()
        self._reviewer = validate_reviewer_identity(reviewer) if reviewer else ""
        self._request_token = secrets.token_urlsafe(32)

    @property
    def reviewer(self) -> str:
        with self._lock:
            return self._reviewer

    @property
    def request_token(self) -> str:
        return self._request_token

    def set_reviewer(self, reviewer: str) -> str:
        candidate = validate_reviewer_identity(reviewer)
        with self._lock:
            if self._reviewer and self._reviewer != candidate:
                raise ValueError("reviewer identity is fixed for this server session")
            self._reviewer = candidate
            return self._reviewer

    def require_reviewer(self) -> str:
        reviewer = self.reviewer
        if not reviewer:
            raise ValueError("reviewer identity is required before recording decisions")
        return reviewer


def has_review_provenance(labels: dict) -> bool:
    reviewed_at = str(labels.get("reviewed_at", "")).strip()
    try:
        timestamp = datetime.fromisoformat(reviewed_at.replace("Z", "+00:00"))
    except ValueError:
        return False
    return (
        labels.get("review_method") == "human"
        and labels.get("reviewed") is True
        and labels.get("review_attestation") == HUMAN_REVIEW_ATTESTATION
        and bool(str(labels.get("reviewed_by", "")).strip())
        and timestamp.tzinfo is not None
    )


def is_human_review(labels: dict) -> bool:
    return has_review_provenance(labels) and labels.get("excluded") is not True


def is_excluded_review(labels: dict) -> bool:
    return has_review_provenance(labels) and labels.get("excluded") is True


def review_queue(root: Path, target_groups_per_placement: int = 10,
                 target_groups_per_preservation_kind: int = 3,
                 target_groups_per_negative_placement: int = 3) -> list[dict]:
    records = []
    # The queue exists to guide *human* review.  Proposed AI boxes can make a
    # frame quick to check, but must not make a placement look covered before
    # a reviewer actually approves an independent source group.
    human_placement_groups: dict[str, set[str]] = defaultdict(set)
    human_negative_placement_groups: dict[str, set[str]] = defaultdict(set)
    human_preservation_groups: dict[str, set[str]] = defaultdict(set)
    human_reviewed_groups: set[str] = set()
    for manifest in sorted(root.rglob("manifest.jsonl")):
        for line in manifest.read_text().splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            image = manifest.parent / record["local_path"]
            labels_path = image.with_suffix(image.suffix + ".labels.json")
            labels = json.loads(labels_path.read_text()) if labels_path.exists() else {"boxes": [], "reviewed": False}
            group = record.get("split_group") or record.get("source_page") or record.get("sha256")
            if is_human_review(labels):
                human_reviewed_groups.add(group)
                for placement in {box.get("placement", "unknown") for box in labels.get("boxes", [])}:
                    human_placement_groups[placement].add(group)
                for placement in labels.get("negative_placements", []):
                    human_negative_placement_groups[placement].add(group)
                for kind in {region.get("kind", "unknown")
                             for region in labels.get("preserve_regions", [])}:
                    human_preservation_groups[kind].add(group)
            records.append((record, image, labels, group))

    items = []
    keyword_placements = {
        "jersey": ("jersey", "shirt", "kit"),
        "venue_board": ("stadium", "venue", "pitchside", "advertising board", "hoarding"),
        "broadcast_overlay": ("broadcast", "television", "scorebug", "overlay"),
        "ordinary_screen": ("screen", "monitor", "display", "billboard"),
        "car_livery": ("nascar", "formula one", "race car", "livery", "motogp"),
    }
    for record, image, labels, group in records:
        if image.suffix.lower() not in IMAGE_EXTENSIONS or not image.is_file():
            continue
        if is_human_review(labels) or is_excluded_review(labels):
            continue
        boxes = labels.get("boxes", [])
        placements = {box.get("placement", "unknown") for box in boxes}
        context = f"{record.get('depicted_context', '')} {record.get('title', '')}".casefold()
        if not placements:
            placements = {
                placement for placement, keywords in keyword_placements.items()
                if any(keyword in context for keyword in keywords)
            }
        reasons = []
        priority = 0
        for placement in sorted(placements):
            group_count = len(human_placement_groups.get(placement, set()))
            deficit = max(0, target_groups_per_placement - group_count)
            if deficit:
                priority += deficit
                reasons.append(f"{placement}: {group_count}/{target_groups_per_placement} groups")
        for placement in sorted(set(labels.get("negative_placements", []))):
            group_count = len(human_negative_placement_groups.get(placement, set()))
            deficit = max(0, target_groups_per_negative_placement - group_count)
            if deficit:
                priority += deficit
                reasons.append(
                    f"hard negative {placement}: {group_count}/"
                    f"{target_groups_per_negative_placement} groups"
                )
        for kind in sorted({region.get("kind", "unknown")
                            for region in labels.get("preserve_regions", [])}):
            group_count = len(human_preservation_groups.get(kind, set()))
            deficit = max(0, target_groups_per_preservation_kind - group_count)
            if deficit:
                priority += deficit
                reasons.append(
                    f"preserve {kind}: {group_count}/{target_groups_per_preservation_kind} groups"
                )
        if labels.get("review_method") == "independent_visual_ai_review":
            priority += 3
            reasons.append("AI boxes ready to verify")
        elif not boxes:
            priority += 2
            reasons.append("needs annotation or hard-negative confirmation")
        if group in human_reviewed_groups:
            reasons.append("split group already has human approval")
        items.append({
            "labels": labels,
            "license": record.get("license", ""),
            "path": str(image.relative_to(root)),
            "priority": priority,
            "priority_reasons": reasons,
            "group_has_human_review": group in human_reviewed_groups,
            "source_group": group,
            "source_page": record.get("source_page", ""),
            "title": record.get("title", image.name),
        })

    # Several images from a match or photo sequence add training variety but
    # only one independent source group.  Present the strongest candidate from
    # each group first, then defer siblings until the reviewer has covered the
    # other groups.  The UI reloads this queue after every approval, so the
    # ordering immediately reflects the newly approved group.
    groups: dict[str, list[dict]] = defaultdict(list)
    for item in items:
        groups[item["source_group"]].append(item)
    for grouped_items in groups.values():
        grouped_items.sort(key=lambda item: (-item["priority"], item["path"]))
        for position, item in enumerate(grouped_items):
            item["group_position"] = position + 1
            if position:
                item["priority_reasons"].append(
                    "same split group; review after independent groups"
                )

    return sorted(items, key=lambda item: (
        item["group_has_human_review"],
        item["group_position"] > 1,
        -item["priority"],
        item["path"],
    ))


def review_summary(root: Path) -> dict:
    groups: set[str] = set()
    human_groups: set[str] = set()
    placement_groups: dict[str, set[str]] = defaultdict(set)
    human_placement_groups: dict[str, set[str]] = defaultdict(set)
    negative_placement_groups: dict[str, set[str]] = defaultdict(set)
    human_negative_placement_groups: dict[str, set[str]] = defaultdict(set)
    preservation_groups: dict[str, set[str]] = defaultdict(set)
    human_preservation_groups: dict[str, set[str]] = defaultdict(set)
    images = 0
    human_images = 0
    excluded_images = 0
    for manifest in sorted(root.rglob("manifest.jsonl")):
        for line in manifest.read_text().splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            image = manifest.parent / record["local_path"]
            if not image.is_file():
                continue
            images += 1
            group = record.get("split_group") or record.get("source_page") or record.get("sha256")
            groups.add(group)
            labels_path = image.with_suffix(image.suffix + ".labels.json")
            labels = json.loads(labels_path.read_text()) if labels_path.exists() else {}
            human = is_human_review(labels)
            if is_excluded_review(labels):
                excluded_images += 1
            if human:
                human_images += 1
                human_groups.add(group)
            for placement in {box.get("placement", "unknown") for box in labels.get("boxes", [])}:
                placement_groups[placement].add(group)
                if human:
                    human_placement_groups[placement].add(group)
            for placement in set(labels.get("negative_placements", [])):
                negative_placement_groups[placement].add(group)
                if human:
                    human_negative_placement_groups[placement].add(group)
            for kind in {region.get("kind", "unknown")
                         for region in labels.get("preserve_regions", [])}:
                preservation_groups[kind].add(group)
                if human:
                    human_preservation_groups[kind].add(group)
    return {
        "excluded_images": excluded_images,
        "human_images": human_images,
        "human_source_groups": len(human_groups),
        "images": images,
        "pending_images": len(review_queue(root)),
        "negative_placement_source_groups": {
            key: {"all_review_methods": len(values),
                  "human": len(human_negative_placement_groups.get(key, set()))}
            for key, values in sorted(negative_placement_groups.items())
        },
        "placement_source_groups": {
            key: {"all_review_methods": len(values),
                  "human": len(human_placement_groups.get(key, set()))}
            for key, values in sorted(placement_groups.items())
        },
        "preservation_source_groups": {
            key: {"all_review_methods": len(values),
                  "human": len(human_preservation_groups.get(key, set()))}
            for key, values in sorted(preservation_groups.items())
        },
        "source_groups": len(groups),
    }


def review_plan(root: Path, target_groups: int = 3) -> dict:
    """Greedily choose independent pending groups that cover promotion facets."""
    required = {
        *(("placement", value) for value in REQUIRED_PROMOTION_PLACEMENTS),
        *(("preservation", value) for value in REQUIRED_PROMOTION_PRESERVATION_KINDS),
        *(("negative", value) for value in REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS),
    }
    summary = review_summary(root)
    counts = {}
    for kind, value in required:
        section = {
            'placement': 'placement_source_groups',
            'preservation': 'preservation_source_groups',
            'negative': 'negative_placement_source_groups',
        }[kind]
        counts[(kind, value)] = summary.get(section, {}).get(value, {}).get('human', 0)

    groups: dict[str, list[dict]] = defaultdict(list)
    for item in review_queue(root):
        labels = item['labels']
        facets = {
            *(('placement', box.get('placement', 'unknown')) for box in labels.get('boxes', [])),
            *(('preservation', region.get('kind', 'unknown'))
              for region in labels.get('preserve_regions', [])),
            *(('negative', placement) for placement in labels.get('negative_placements', [])),
        }
        groups[item['source_group']].append({
            'facets': facets, 'path': item['path'], 'priority': item['priority'],
        })

    selected = []
    remaining = dict(groups)
    while any(counts[facet] < target_groups for facet in required):
        ranked = []
        for key, candidates in remaining.items():
            for candidate in candidates:
                useful = sorted(
                    facet for facet in candidate['facets']
                    if facet in required and counts[facet] < target_groups
                )
                if useful:
                    ranked.append((len(useful), candidate['priority'], key,
                                   candidate['path'], useful, candidate))
        if not ranked:
            break
        _, _, key, path, useful, candidate = max(
            ranked, key=lambda row: (row[0], row[1], row[2], row[3])
        )
        selected.append({
            'covers': [f'{kind}:{value}' for kind, value in useful],
            'path': path,
            'source_group': key,
        })
        for facet in candidate['facets']:
            if facet in counts:
                counts[facet] += 1
        # One representative per independent group keeps the plan short and
        # prevents related frames from masquerading as additional evidence.
        remaining.pop(key)

    return {
        'remaining_deficits': {
            f'{kind}:{value}': max(0, target_groups - counts[(kind, value)])
            for kind, value in sorted(required)
            if counts[(kind, value)] < target_groups
        },
        'selected_group_count': len(selected),
        'selected_groups': selected,
        'target_groups_per_facet': target_groups,
    }


def load_plan_details(plan_path: Path) -> dict[str, list[str]]:
    payload = json.loads(plan_path.read_text())
    groups = payload.get("selected_groups")
    if not isinstance(groups, list):
        raise ValueError("review plan must contain selected_groups")
    details: dict[str, list[str]] = {}
    for item in groups:
        if not isinstance(item, dict) or not isinstance(item.get("path"), str) or not item["path"]:
            raise ValueError("every selected review group must have a non-empty path")
        covers = item.get("covers", [])
        if not isinstance(covers, list) or any(not isinstance(value, str) for value in covers):
            raise ValueError("review plan covers must be a list of strings")
        details[item["path"]] = covers
    return details


def load_plan_paths(plan_path: Path) -> set[str]:
    return set(load_plan_details(plan_path))


def filter_queue(items: list[dict], allowed_paths: set[str] | None,
                 plan_details: dict[str, list[str]] | None = None) -> list[dict]:
    if allowed_paths is None:
        return items
    filtered = [item for item in items if item["path"] in allowed_paths]
    if plan_details is not None:
        for item in filtered:
            item["plan_covers"] = plan_details.get(item["path"], [])
    return filtered


def review_item_revision(root: Path, item: dict) -> str:
    """Bind the exact image, proposal sidecar, and displayed source context."""
    image = resolve_image(root, item["path"])
    labels = image.with_suffix(image.suffix + ".labels.json")
    digest = hashlib.sha256()
    digest.update(b"liveblock-human-review-item-v1\0")
    for value in (
        item["path"], item.get("source_group", ""), item.get("source_page", ""),
        item.get("title", ""), item.get("license", ""),
    ):
        digest.update(str(value).encode())
        digest.update(b"\0")
    digest.update(image.read_bytes())
    digest.update(b"\0")
    if labels.exists():
        digest.update(labels.read_bytes())
    return digest.hexdigest()


def attach_review_revisions(root: Path, items: list[dict]) -> list[dict]:
    for item in items:
        item["review_revision"] = review_item_revision(root, item)
    return items


def review_state_sha256(root: Path) -> str:
    digest = hashlib.sha256()
    paths = sorted([*root.rglob("manifest.jsonl"), *root.rglob("*.labels.json")])
    for path in paths:
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def review_plan_status(root: Path, allowed_paths: set[str],
                       plan_path: Path | None = None) -> dict:
    items = []
    counts = {"approved": 0, "excluded": 0, "missing": 0, "pending": 0}
    for path in sorted(allowed_paths):
        try:
            image = resolve_image(root, path)
        except (ValueError, FileNotFoundError):
            state = "missing"
        else:
            labels_path = image.with_suffix(image.suffix + ".labels.json")
            labels = json.loads(labels_path.read_text()) if labels_path.exists() else {}
            if is_human_review(labels):
                state = "approved"
            elif is_excluded_review(labels):
                state = "excluded"
            else:
                state = "pending"
        counts[state] += 1
        items.append({"path": path, "status": state})
    current_plan = review_plan(root)
    result = {
        "counts": counts,
        "items": items,
        "planned_group_count": len(allowed_paths),
        "remaining_candidate_deficits": current_plan["remaining_deficits"],
        "review_state_sha256": review_state_sha256(root),
    }
    if plan_path is not None:
        result["plan_sha256"] = hashlib.sha256(plan_path.read_bytes()).hexdigest()
    return result


def resolve_image(root: Path, relative_path: str) -> Path:
    candidate = (root / relative_path).resolve()
    resolved_root = root.resolve()
    if candidate.suffix.lower() not in IMAGE_EXTENSIONS or not candidate.is_relative_to(resolved_root):
        raise ValueError("invalid image path")
    if not candidate.is_file():
        raise FileNotFoundError(candidate)
    return candidate


def write_review_document(destination: Path, payload: dict) -> None:
    """Durably replace one regular sidecar from a unique same-directory file."""
    if destination.is_symlink() or (destination.exists() and not destination.is_file()):
        raise ValueError("review sidecar must be a regular non-symlink file")
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{destination.name}.", suffix=".tmp", dir=destination.parent
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, destination)
        try:
            flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
            directory = os.open(destination.parent, flags)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        except OSError:
            # Windows does not expose directory fsync through Python. The file
            # itself was synchronized before the same-directory replacement.
            pass
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def save_review(root: Path, relative_path: str, boxes: list[dict],
                preserve_regions: list[dict] | None = None,
                reviewer: str = "", negative_placements: list[str] | None = None) -> Path:
    reviewer = validate_reviewer_identity(reviewer)
    image = resolve_image(root, relative_path)
    for box in boxes:
        box.pop("confidence", None)
        validate_box(box, image)
    preserve_regions = preserve_regions or []
    negative_placements = sorted(set(negative_placements or []))
    if any(value not in PLACEMENT_KINDS for value in negative_placements):
        raise ValueError(f"negative placements must use {sorted(PLACEMENT_KINDS)}")
    for region in preserve_regions:
        validate_preserve_region(region, image)
    destination = image.with_suffix(image.suffix + ".labels.json")
    write_review_document(destination, {
        "boxes": boxes,
        "negative_placements": negative_placements,
        "preserve_regions": preserve_regions,
        "review_attestation": HUMAN_REVIEW_ATTESTATION,
        "review_method": "human",
        "reviewed": True,
        "reviewed_at": datetime.now(timezone.utc).isoformat(),
        "reviewed_by": reviewer,
    })
    return destination


def save_rejection(root: Path, relative_path: str, reason: str, reviewer: str) -> Path:
    reviewer = validate_reviewer_identity(reviewer)
    reason = reason.strip()
    if not reason:
        raise ValueError("exclusion reason is required")
    image = resolve_image(root, relative_path)
    destination = image.with_suffix(image.suffix + ".labels.json")
    write_review_document(destination, {
        "boxes": [],
        "excluded": True,
        "exclusion_reason": reason,
        "preserve_regions": [],
        "review_attestation": HUMAN_REVIEW_ATTESTATION,
        "review_method": "human",
        "reviewed": True,
        "reviewed_at": datetime.now(timezone.utc).isoformat(),
        "reviewed_by": reviewer,
    })
    return destination


def ui_status(root: Path, allowed_paths: set[str] | None,
              plan_path: Path | None = None) -> dict:
    if allowed_paths is not None:
        return review_plan_status(root, allowed_paths, plan_path)
    summary = review_summary(root)
    return {
        "counts": {
            "approved": summary["human_images"],
            "excluded": summary["excluded_images"],
            "missing": 0,
            "pending": summary["pending_images"],
        },
        "planned_group_count": summary["human_images"] + summary["excluded_images"]
        + summary["pending_images"],
    }


def handler_for(root: Path, session: ReviewSession,
                allowed_paths: set[str] | None = None,
                plan_details: dict[str, list[str]] | None = None,
                plan_path: Path | None = None):
    decision_lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        def require_local_browser(self, require_token: bool = False) -> None:
            port = self.server.server_address[1]
            allowed_hosts = {f"127.0.0.1:{port}", f"localhost:{port}"}
            host = self.headers.get("Host", "").casefold()
            if host not in allowed_hosts:
                raise ValueError("request Host is not the configured loopback reviewer")
            if require_token:
                allowed_origins = {f"http://127.0.0.1:{port}", f"http://localhost:{port}"}
                if self.headers.get("Origin", "").casefold() not in allowed_origins:
                    raise ValueError("request Origin is not the local review page")
                token = self.headers.get("X-LiveBlock-Review-Token", "")
                if not secrets.compare_digest(token, session.request_token):
                    raise ValueError("review request token is missing or invalid")

        def send_bytes(self, status: int, data: bytes, content_type: str) -> None:
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header(
                "Content-Security-Policy",
                "default-src 'self'; img-src 'self'; style-src 'unsafe-inline'; "
                "script-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; "
                "frame-ancestors 'none'",
            )
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self) -> None:  # noqa: N802
            parsed = urllib.parse.urlparse(self.path)
            try:
                self.require_local_browser()
                if parsed.path == "/":
                    self.send_bytes(200, HTML.encode(), "text/html; charset=utf-8")
                elif parsed.path == "/api/items":
                    items = filter_queue(review_queue(root), allowed_paths, plan_details)
                    attach_review_revisions(root, items)
                    self.send_bytes(200, json.dumps(items).encode(), "application/json")
                elif parsed.path == "/api/session":
                    self.send_bytes(200, json.dumps({
                        "reviewer": session.reviewer,
                        "reviewToken": session.request_token,
                    }).encode(), "application/json")
                elif parsed.path == "/api/status":
                    status = ui_status(root, allowed_paths, plan_path)
                    self.send_bytes(200, json.dumps(status).encode(), "application/json")
                elif parsed.path == "/image":
                    relative = urllib.parse.parse_qs(parsed.query).get("path", [""])[0]
                    image = resolve_image(root, relative)
                    self.send_bytes(200, image.read_bytes(), f"image/{image.suffix.lstrip('.')}" )
                else:
                    self.send_bytes(404, b"not found", "text/plain")
            except (ValueError, FileNotFoundError) as error:
                self.send_bytes(400, str(error).encode(), "text/plain")

        def do_POST(self) -> None:  # noqa: N802
            if self.path not in {"/api/session", "/api/label", "/api/reject"}:
                self.send_bytes(404, b"not found", "text/plain"); return
            try:
                self.require_local_browser(require_token=True)
                if self.headers.get_content_type() != "application/json":
                    raise ValueError("requests must use application/json")
                length = int(self.headers.get("Content-Length", "0"))
                if length <= 0 or length > MAX_REQUEST_BYTES:
                    raise ValueError("request body size is invalid")
                payload = json.loads(self.rfile.read(length))
                if not isinstance(payload, dict):
                    raise ValueError("request body must be a JSON object")
                if self.path == "/api/session":
                    reviewer = session.set_reviewer(payload.get("reviewer", ""))
                    self.send_bytes(200, json.dumps({"reviewer": reviewer}).encode(),
                                    "application/json")
                    return
                if allowed_paths is not None and payload.get("path") not in allowed_paths:
                    raise ValueError("image is not part of the configured review plan")
                if payload.get("attested") is not True:
                    raise ValueError("personal full-image review attestation is required")
                reviewer = session.require_reviewer()
                with decision_lock:
                    current_items = filter_queue(review_queue(root), allowed_paths, plan_details)
                    current_item = next(
                        (item for item in current_items if item["path"] == payload.get("path")),
                        None,
                    )
                    if current_item is None:
                        raise ValueError("image is no longer pending human review")
                    expected_revision = review_item_revision(root, current_item)
                    if not secrets.compare_digest(
                        str(payload.get("review_revision", "")), expected_revision
                    ):
                        raise ValueError("review item changed after it was displayed; reload it")
                    image = resolve_image(root, payload["path"])
                    sidecar = image.with_suffix(image.suffix + ".labels.json")
                    current = json.loads(sidecar.read_text()) if sidecar.exists() else {}
                    if has_review_provenance(current):
                        raise ValueError("image already has an attributable human decision")
                    if self.path == "/api/reject":
                        destination = save_rejection(root, payload["path"],
                                                     payload.get("reason", ""), reviewer)
                    else:
                        destination = save_review(
                            root, payload["path"], payload.get("boxes", []),
                            payload.get("preserve_regions", []), reviewer,
                            payload.get("negative_placements", []),
                        )
                self.send_bytes(200, json.dumps({"saved": str(destination)}).encode(), "application/json")
            except (ValueError, KeyError, json.JSONDecodeError) as error:
                self.send_bytes(400, str(error).encode(), "text/plain")

        def log_message(self, fmt: str, *args) -> None:
            print(fmt % args)

    return Handler


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pool", type=Path, required=True)
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument(
        "--reviewer",
        help="optional reviewer name/identifier; otherwise the local UI asks once",
    )
    parser.add_argument(
        "--no-open-browser", action="store_true",
        help="do not open the loopback review page automatically",
    )
    parser.add_argument("--list", action="store_true", help="print pending queue JSON and exit")
    parser.add_argument("--summary", action="store_true", help="print human/candidate group coverage and exit")
    parser.add_argument("--plan", action="store_true", help="print a minimal-priority human review plan and exit")
    parser.add_argument("--plan-file", type=Path,
                        help="serve only selected_groups from an existing review plan JSON")
    parser.add_argument("--plan-status", action="store_true",
                        help="report approval status and current deficits for --plan-file")
    args = parser.parse_args()
    try:
        plan_details = load_plan_details(args.plan_file) if args.plan_file else None
        allowed_paths = set(plan_details) if plan_details is not None else None
    except (OSError, ValueError, json.JSONDecodeError) as error:
        parser.error(f"invalid --plan-file: {error}")
    if args.summary:
        print(json.dumps(review_summary(args.pool), indent=2, sort_keys=True))
        return 0
    if args.plan:
        print(json.dumps(review_plan(args.pool), indent=2, sort_keys=True))
        return 0
    if args.plan_status:
        if allowed_paths is None:
            parser.error("--plan-status requires --plan-file")
        print(json.dumps(review_plan_status(args.pool, allowed_paths, args.plan_file),
                         indent=2, sort_keys=True))
        return 0
    if args.list:
        print(json.dumps(filter_queue(review_queue(args.pool), allowed_paths, plan_details),
                         indent=2, sort_keys=True))
        return 0
    try:
        session = ReviewSession(args.reviewer)
    except ValueError as error:
        parser.error(str(error))
    pending = filter_queue(review_queue(args.pool), allowed_paths)
    server = ThreadingHTTPServer(
        ("127.0.0.1", args.port),
        handler_for(args.pool, session, allowed_paths, plan_details, args.plan_file),
    )
    scope = "planned" if allowed_paths is not None else "pending"
    url = f"http://127.0.0.1:{args.port}"
    print(f"Review UI: {url} ({len(pending)} {scope})")
    if not args.no_open_browser:
        threading.Timer(0.2, lambda: webbrowser.open(url)).start()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nReview server stopped")
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
