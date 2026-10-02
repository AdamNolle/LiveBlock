#!/usr/bin/env python3
"""Create a local YOLO ad dataset from CC BY 4.0 AdSERP public screenshots.

Source: https://zenodo.org/records/15236546 (Latifzadeh, Gwizdka, Leiva, 2025).
Only screenshot, annotation and query metadata archives are used. Splits are
grouped by query, so repeats/crops of the same search cannot leak into validation.
This dataset covers search ads; it does not establish sports-logo performance.
"""
from __future__ import annotations
import argparse
import hashlib
import io
import json
import math
from pathlib import Path
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
import zipfile
from PIL import Image

FILES = {
    "ad-boundary-data.zip": "d90314b5bb71388f85f69834c6772c8a",
    "trial-metadata.zip": "a9e05935db7d1d1f697cbb8648336654",
    "full-page-screenshots.zip": "ecb0b3b15cf4ce5b30731d7b5c1ceb33",
}


def checksum(path: Path) -> str:
    digest = hashlib.md5()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""): digest.update(block)
    return digest.hexdigest()


def split_for_query(query: str) -> str:
    bucket = int(hashlib.sha256(("liveblock-adserp-v1:" + query).encode()).hexdigest()[:8], 16) % 100
    return "train" if bucket < 70 else "val" if bucket < 85 else "test"


def crop_labels(annotations: dict, width: int, height: int, top: int, crop_height: int) -> list[str]:
    rows = []
    for kind in ("native_ad", "dd_top", "dd_right"):
        for box in annotations.get(kind, []):
            x, y = float(box["location"]["x"]), float(box["location"]["y"])
            w, h = float(box["size"]["width"]), float(box["size"]["height"])
            if not all(math.isfinite(v) for v in (x, y, w, h)) or min(w, h) <= 0: continue
            x0, x1 = max(0.0, x), min(float(width), x + w)
            y0, y1 = max(float(top), y), min(float(top + crop_height), y + h, float(height))
            if x1 <= x0 or y1 <= y0: continue
            # Label even a partial ad: it is still visible content to block.
            rows.append(f"0 {(x0+x1)/2/width:.6f} {((y0+y1)/2-top)/crop_height:.6f} {(x1-x0)/width:.6f} {(y1-y0)/crop_height:.6f}")
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parent / "datasets/adserp/source")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent / "datasets/adserp/yolo")
    parser.add_argument("--limit-pages", type=int, default=900)
    parser.add_argument("--viewport-height", type=int, default=900)
    parser.add_argument("--download", action="store_true", help="Download the three verified public archives from Zenodo")
    args = parser.parse_args()
    if args.limit_pages < 1 or args.viewport_height < 128: parser.error("invalid page limit or viewport height")
    if args.output.exists(): parser.error(f"Output already exists: {args.output}")
    if args.download:
        args.source.mkdir(parents=True, exist_ok=True)
        for name, expected in FILES.items():
            destination = args.source / name
            if destination.exists() and checksum(destination) == expected: continue
            temporary = destination.with_suffix(destination.suffix + ".part")
            url = f"https://zenodo.org/records/15236546/files/{name}?download=1"
            print(f"Downloading {name} from Zenodo", flush=True)
            with urllib.request.urlopen(url, timeout=120) as response, temporary.open("wb") as output:
                for chunk in iter(lambda: response.read(2 * 1024 * 1024), b""): output.write(chunk)
            if checksum(temporary) != expected: raise RuntimeError(f"Archive checksum mismatch: {name}")
            temporary.replace(destination)
    for name, expected in FILES.items():
        path = args.source / name
        if not path.exists() or checksum(path) != expected:
            parser.error(f"Missing or checksum-mismatched archive: {path}")
        print(f"Verified {name}", flush=True)
    with zipfile.ZipFile(args.source / "ad-boundary-data.zip") as archive:
        annotations = {Path(name).stem: json.loads(archive.read(name)) for name in archive.namelist()
            if name.endswith(".json") and "__MACOSX" not in name}
    with zipfile.ZipFile(args.source / "trial-metadata.zip") as archive:
        queries = {}
        for name in archive.namelist():
            if not name.endswith(".xml") or "__MACOSX" in name: continue
            root = ET.fromstring(archive.read(name))
            query = urllib.parse.parse_qs(urllib.parse.urlparse(root.findtext("url", "")).query).get("q", [""])[0]
            if query: queries[Path(name).stem] = query
    for split in ("train", "val", "test"):
        for folder in ("images", "labels"): (args.output / folder / split).mkdir(parents=True)
    report = {"source": "https://zenodo.org/records/15236546", "license": "CC BY 4.0",
        "citation": "Kayhan Latifzadeh, Jacek Gwizdka, Luis A. Leiva. 2025. A Versatile Dataset of Mouse and Eye Movements on Search Engine Results Pages. SIGIR 2025. DOI:10.1145/3726302.3730325",
        "scope": "Search result ads only; no sports/team/number labels.",
        "archives": FILES, "split_method": "SHA-256 of query, 70/15/15; one page per query",
        "counts": {s: {"images": 0, "boxes": 0, "negatives": 0} for s in ("train", "val", "test")}, "pages": []}
    used_queries, used_crops = set(), set()
    with zipfile.ZipFile(args.source / "full-page-screenshots.zip") as archive:
        for name in sorted(archive.namelist()):
            if Path(name).suffix.lower() not in (".png", ".jpg", ".jpeg") or "__MACOSX" in name: continue
            stem = Path(name).stem
            if stem not in annotations or stem not in queries: continue
            query = queries[stem]
            if query in used_queries: continue
            used_queries.add(query)
            split = split_for_query(query)
            image = Image.open(io.BytesIO(archive.read(name))).convert("RGB")
            width, height = image.size
            view_h = min(args.viewport_height, height)
            offsets = sorted(set([*range(0, max(1, height-view_h+1), max(1, view_h-180)), max(0, height-view_h)]))
            for top in offsets:
                crop = image.crop((0, top, width, top+view_h))
                digest = hashlib.sha256(crop.tobytes()).hexdigest()
                if digest in used_crops: continue
                used_crops.add(digest)
                rows = crop_labels(annotations[stem], width, height, top, view_h)
                output_stem = f"{stem}-y{top}"
                crop.save(args.output / "images" / split / f"{output_stem}.jpg", quality=93)
                (args.output / "labels" / split / f"{output_stem}.txt").write_text("\n".join(rows) + ("\n" if rows else ""))
                counts = report["counts"][split]; counts["images"] += 1; counts["boxes"] += len(rows); counts["negatives"] += not rows
            report["pages"].append({"source_file": name, "query": query, "split": split})
            if len(used_queries) % 50 == 0: print(f"Prepared {len(used_queries)} unique search pages", flush=True)
            if len(used_queries) >= args.limit_pages: break
    for split, count in report["counts"].items():
        if count["images"] == 0 or count["boxes"] == 0: raise RuntimeError(f"Empty or unlabeled {split} split")
    (args.output / "provenance.json").write_text(json.dumps(report, indent=2) + "\n")
    (args.output / "ATTRIBUTION.md").write_text(f"# AdSERP source and attribution\n\n{report['citation']}\n\nSource: {report['source']}\n\nLicense: Creative Commons Attribution 4.0 International, https://creativecommons.org/licenses/by/4.0/\n\nChanges: one page per query, viewport crops, conversion of public ad boundaries to normalized YOLO labels, query-grouped train/validation/test splits.\n\n{report['scope']}\n")
    (args.output / "data.yaml").write_text(f"path: {json.dumps(str(args.output.resolve()))}\ntrain: images/train\nval: images/val\ntest: images/test\nnames:\n  0: ad\n")
    print(json.dumps(report["counts"], indent=2)); print(args.output / "data.yaml")
    return 0


if __name__ == "__main__": raise SystemExit(main())
