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
import urllib.parse
from collections import defaultdict
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from corpus.build_sports_corpus import (PLACEMENT_KINDS,
                                        REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
                                        REQUIRED_PROMOTION_PLACEMENTS,
                                        REQUIRED_PROMOTION_PRESERVATION_KINDS,
                                        validate_box, validate_preserve_region)

IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}
HTML = r"""<!doctype html><meta charset="utf-8"><title>LiveBlock corpus review</title>
<style>
body{font:14px system-ui;background:#111;color:#eee;margin:0}header{position:sticky;top:0;background:#1b1b1b;padding:10px;display:flex;gap:8px;align-items:center;z-index:2}button,select{font:inherit;padding:6px}#stage{position:relative;margin:16px auto;width:min(94vw,1200px)}img{display:block;width:100%;user-select:none}canvas{position:absolute;inset:0;width:100%;height:100%;cursor:crosshair}.meta{margin-left:auto;color:#aaa}#help{padding:0 16px 16px;color:#aaa}
</style>
<header><button id="prev">←</button><button id="next">→</button><select id="mode"><option value="block">Block sponsor</option><option value="keep">Keep team identity</option></select><select id="cls"><option>Logo</option><option>Ad banner</option><option>Sponsored</option></select><select id="place"><option>car_livery</option><option>jersey</option><option>venue_board</option><option>broadcast_overlay</option><option>ordinary_screen</option><option>helmet</option></select><select id="keep"><option value="team_name">team name</option><option value="jersey_number">jersey number</option><option value="vehicle_number">vehicle number</option><option value="team_crest">team crest</option><option value="manufacturer_badge">manufacturer badge</option></select><span id="negative">Hard negative: <label><input type="checkbox" value="ordinary_screen">screen</label><label><input type="checkbox" value="broadcast_overlay">broadcast</label><label><input type="checkbox" value="jersey">jersey</label><label><input type="checkbox" value="car_livery">car</label><label><input type="checkbox" value="venue_board">venue</label><label><input type="checkbox" value="helmet">helmet</label></span><button id="undo">Delete last</button><button id="approve">Approve human review</button><button id="reject">Reject unusable</button><span class="meta" id="meta"></span></header>
<div id="stage"><img id="image"><canvas id="canvas"></canvas></div><div id="help">Choose “Block sponsor” only for paid brand marks, ads, and sponsorship labels. Choose “Keep team identity” for team names, jersey numbers, and team crests. Drag to add a box; click an existing box to delete it. Approval writes the sidecar atomically. Keyboard: ←/→ navigate, Ctrl/Cmd+Enter approve.</div>
<script>
let items=[], index=0, boxes=[], preserveRegions=[], negativePlacements=[], drag=null,lastAdded=null;const img=document.querySelector('#image'),canvas=document.querySelector('#canvas'),ctx=canvas.getContext('2d'),meta=document.querySelector('#meta');
async function boot(){items=await (await fetch('/api/items')).json();show(0)}
function show(i){if(!items.length){meta.textContent='No pending items';return}index=(i+items.length)%items.length;let it=items[index];boxes=structuredClone(it.labels.boxes||[]);preserveRegions=structuredClone(it.labels.preserve_regions||[]);negativePlacements=structuredClone(it.labels.negative_placements||[]);document.querySelectorAll('#negative input').forEach(input=>input.checked=negativePlacements.includes(input.value));lastAdded=null;img.src='/image?path='+encodeURIComponent(it.path);let covers=(it.plan_covers||[]).length?` · PLAN: ${it.plan_covers.join(', ')}`:'';meta.textContent=`${index+1}/${items.length} · priority ${it.priority} · ${it.path} · ${it.labels.review_method||'unreviewed'}${covers} · ${it.priority_reasons.join('; ')}`;img.onload=resize}
function resize(){canvas.width=img.naturalWidth;canvas.height=img.naturalHeight;draw()}
function draw(){ctx.clearRect(0,0,canvas.width,canvas.height);ctx.lineWidth=Math.max(2,canvas.width/600);boxes.forEach((b,i)=>{ctx.strokeStyle=['#00ff88','#ffcc00','#ff55cc'][['Logo','Ad banner','Sponsored'].indexOf(b.class)]||'#fff';ctx.strokeRect(b.x*canvas.width,b.y*canvas.height,b.width*canvas.width,b.height*canvas.height);ctx.fillStyle=ctx.strokeStyle;ctx.fillText(`${i+1} ${b.class} · ${b.placement}`,b.x*canvas.width+3,b.y*canvas.height+14)});preserveRegions.forEach((b,i)=>{ctx.strokeStyle='#55ddff';ctx.strokeRect(b.x*canvas.width,b.y*canvas.height,b.width*canvas.width,b.height*canvas.height);ctx.fillStyle=ctx.strokeStyle;ctx.fillText(`KEEP ${i+1} · ${b.kind}`,b.x*canvas.width+3,b.y*canvas.height+14)});if(drag){ctx.strokeStyle='#fff';ctx.strokeRect(drag.x,drag.y,drag.w,drag.h)}}
function point(e){let r=canvas.getBoundingClientRect();return{x:(e.clientX-r.left)*canvas.width/r.width,y:(e.clientY-r.top)*canvas.height/r.height}}
canvas.onmousedown=e=>{let p=point(e);let hit=boxes.findIndex(b=>p.x>=b.x*canvas.width&&p.x<=(b.x+b.width)*canvas.width&&p.y>=b.y*canvas.height&&p.y<=(b.y+b.height)*canvas.height);if(hit>=0){boxes.splice(hit,1);draw();return}let keepHit=preserveRegions.findIndex(b=>p.x>=b.x*canvas.width&&p.x<=(b.x+b.width)*canvas.width&&p.y>=b.y*canvas.height&&p.y<=(b.y+b.height)*canvas.height);if(keepHit>=0){preserveRegions.splice(keepHit,1);draw();return}drag={x:p.x,y:p.y,w:0,h:0}}
canvas.onmousemove=e=>{if(!drag)return;let p=point(e);drag.w=p.x-drag.x;drag.h=p.y-drag.y;draw()}
canvas.onmouseup=()=>{if(!drag)return;let x=Math.min(drag.x,drag.x+drag.w),y=Math.min(drag.y,drag.y+drag.h),w=Math.abs(drag.w),h=Math.abs(drag.h);if(w>3&&h>3){let region={x:x/canvas.width,y:y/canvas.height,width:w/canvas.width,height:h/canvas.height};if(document.querySelector('#mode').value==='keep'){preserveRegions.push({kind:document.querySelector('#keep').value,...region});lastAdded='keep'}else{boxes.push({class:document.querySelector('#cls').value,placement:document.querySelector('#place').value,...region});lastAdded='block'}}drag=null;draw()}
document.querySelector('#prev').onclick=()=>show(index-1);document.querySelector('#next').onclick=()=>show(index+1);document.querySelector('#undo').onclick=()=>{if(lastAdded==='keep')preserveRegions.pop();else boxes.pop();lastAdded=null;draw()};
async function approve(){negativePlacements=[...document.querySelectorAll('#negative input:checked')].map(input=>input.value);let response=await fetch('/api/label',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:items[index].path,boxes,preserve_regions:preserveRegions,negative_placements:negativePlacements})});if(!response.ok){alert(await response.text());return}await boot()}
async function reject(){let reason=prompt('Why is this image unusable for the corpus?');if(!reason)return;let response=await fetch('/api/reject',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:items[index].path,reason})});if(!response.ok){alert(await response.text());return}await boot()}
document.querySelector('#approve').onclick=approve;document.querySelector('#reject').onclick=reject;onkeydown=e=>{if(e.key==='ArrowLeft')show(index-1);if(e.key==='ArrowRight')show(index+1);if(e.key==='Enter'&&(e.ctrlKey||e.metaKey))approve()};boot();
</script>"""


def has_review_provenance(labels: dict) -> bool:
    reviewed_at = str(labels.get("reviewed_at", "")).strip()
    try:
        timestamp = datetime.fromisoformat(reviewed_at.replace("Z", "+00:00"))
    except ValueError:
        return False
    return (
        labels.get("review_method") == "human"
        and labels.get("reviewed") is True
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


def save_review(root: Path, relative_path: str, boxes: list[dict],
                preserve_regions: list[dict] | None = None,
                reviewer: str = "", negative_placements: list[str] | None = None) -> Path:
    reviewer = reviewer.strip()
    if not reviewer:
        raise ValueError("reviewer identity is required")
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
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    temporary.write_text(json.dumps({
        "boxes": boxes,
        "negative_placements": negative_placements,
        "preserve_regions": preserve_regions,
        "review_method": "human",
        "reviewed": True,
        "reviewed_at": datetime.now(timezone.utc).isoformat(),
        "reviewed_by": reviewer,
    }, indent=2, sort_keys=True) + "\n")
    os.replace(temporary, destination)
    return destination


def save_rejection(root: Path, relative_path: str, reason: str, reviewer: str) -> Path:
    reviewer = reviewer.strip()
    reason = reason.strip()
    if not reviewer:
        raise ValueError("reviewer identity is required")
    if not reason:
        raise ValueError("exclusion reason is required")
    image = resolve_image(root, relative_path)
    destination = image.with_suffix(image.suffix + ".labels.json")
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    temporary.write_text(json.dumps({
        "boxes": [],
        "excluded": True,
        "exclusion_reason": reason,
        "preserve_regions": [],
        "review_method": "human",
        "reviewed": True,
        "reviewed_at": datetime.now(timezone.utc).isoformat(),
        "reviewed_by": reviewer,
    }, indent=2, sort_keys=True) + "\n")
    os.replace(temporary, destination)
    return destination


def handler_for(root: Path, reviewer: str, allowed_paths: set[str] | None = None,
                plan_details: dict[str, list[str]] | None = None):
    class Handler(BaseHTTPRequestHandler):
        def send_bytes(self, status: int, data: bytes, content_type: str) -> None:
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self) -> None:  # noqa: N802
            parsed = urllib.parse.urlparse(self.path)
            try:
                if parsed.path == "/":
                    self.send_bytes(200, HTML.encode(), "text/html; charset=utf-8")
                elif parsed.path == "/api/items":
                    items = filter_queue(review_queue(root), allowed_paths, plan_details)
                    self.send_bytes(200, json.dumps(items).encode(), "application/json")
                elif parsed.path == "/image":
                    relative = urllib.parse.parse_qs(parsed.query).get("path", [""])[0]
                    image = resolve_image(root, relative)
                    self.send_bytes(200, image.read_bytes(), f"image/{image.suffix.lstrip('.')}" )
                else:
                    self.send_bytes(404, b"not found", "text/plain")
            except (ValueError, FileNotFoundError) as error:
                self.send_bytes(400, str(error).encode(), "text/plain")

        def do_POST(self) -> None:  # noqa: N802
            if self.path not in {"/api/label", "/api/reject"}:
                self.send_bytes(404, b"not found", "text/plain"); return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                payload = json.loads(self.rfile.read(length))
                if allowed_paths is not None and payload.get("path") not in allowed_paths:
                    raise ValueError("image is not part of the configured review plan")
                if self.path == "/api/reject":
                    destination = save_rejection(root, payload["path"], payload.get("reason", ""), reviewer)
                else:
                    destination = save_review(root, payload["path"], payload.get("boxes", []),
                                              payload.get("preserve_regions", []), reviewer,
                                              payload.get("negative_placements", []))
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
    parser.add_argument("--reviewer", help="reviewer name or identifier recorded in every approval")
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
    if not args.reviewer or not args.reviewer.strip():
        parser.error("--reviewer is required when starting the review server")
    pending = filter_queue(review_queue(args.pool), allowed_paths)
    server = ThreadingHTTPServer(("127.0.0.1", args.port),
                                 handler_for(args.pool, args.reviewer, allowed_paths, plan_details))
    scope = "planned" if allowed_paths is not None else "pending"
    print(f"Review UI: http://127.0.0.1:{args.port} ({len(pending)} {scope})")
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
