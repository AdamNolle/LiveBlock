#!/usr/bin/env python3
"""Bundle the verified CPU ONNX Runtime 1.24.4 library for a desktop target.

Use --platform windows --arch x64, or --platform linux --arch x64/aarch64.
GPU deployments may supply a compatible vendor runtime through ORT_DYLIB_PATH.
"""
from __future__ import annotations
import argparse
import hashlib
import io
import json
from pathlib import Path
import platform
import tarfile
import urllib.request
import zipfile

VERSION = "1.24.4"
HASHES = {
    ("windows", "x64"): "d2319fddfb6ea4db99ccc4b60c85c517bcd855721f5daa6a06d40d7cb2ee2357",
    ("linux", "x64"): "3a211fbea252c1e66290658f1b735b772056149f28321e71c308942cdb54b747",
    ("linux", "aarch64"): "866109a9248d057671a039b9d725be4bd86888e3754140e6701ec621be9d4d7e",
}
ROOT = Path(__file__).resolve().parent.parent


def target_name(name: str, target: str) -> str | None:
    basename = Path(name).name
    if basename in ("LICENSE", "ThirdPartyNotices.txt"):
        return "onnxruntime-" + basename
    if target == "windows" and basename.startswith("onnxruntime") and basename.endswith(".dll"):
        return basename
    if target == "linux":
        if basename == f"libonnxruntime.so.{VERSION}": return "libonnxruntime.so"
        if basename.startswith("libonnxruntime_providers") and basename.endswith(".so"): return basename
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=["windows", "linux"], required=True)
    parser.add_argument("--arch", choices=["x64", "aarch64"], default="aarch64" if platform.machine() in ("arm64", "aarch64") else "x64")
    args = parser.parse_args()
    expected = HASHES.get((args.platform, args.arch))
    if not expected: parser.error("Unsupported target architecture")
    suffix = "zip" if args.platform == "windows" else "tgz"
    system = "win" if args.platform == "windows" else "linux"
    archive_name = f"onnxruntime-{system}-{args.arch}-{VERSION}.{suffix}"
    cache = ROOT / "tools/runtime-downloads" / archive_name
    cache.parent.mkdir(parents=True, exist_ok=True)
    url = f"https://github.com/microsoft/onnxruntime/releases/download/v{VERSION}/{archive_name}"
    if not cache.exists(): urllib.request.urlretrieve(url, cache)
    payload = cache.read_bytes()
    if hashlib.sha256(payload).hexdigest() != expected: raise RuntimeError(f"Checksum mismatch: {cache}")
    destination = ROOT / "platform" / args.platform / "src-tauri/resources"
    destination.mkdir(parents=True, exist_ok=True)
    selected = {}
    if suffix == "zip":
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            for name in archive.namelist():
                output = target_name(name, args.platform)
                if output: selected[output] = archive.read(name)
    else:
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as archive:
            for member in archive:
                output = target_name(member.name, args.platform)
                if output and member.isfile():
                    stream = archive.extractfile(member)
                    if stream: selected[output] = stream.read()
    required = "onnxruntime.dll" if args.platform == "windows" else "libonnxruntime.so"
    if required not in selected: raise RuntimeError(f"No runtime library in {archive_name}")
    for name, data in selected.items(): (destination / name).write_bytes(data)
    (destination / "onnxruntime-build.json").write_text(json.dumps({
        "version": VERSION, "platform": args.platform, "arch": args.arch, "source": url,
        "archive_sha256": expected, "execution_provider": "CPU",
    }, indent=2) + "\n")
    print(f"Bundled verified {args.platform}/{args.arch} runtime in {destination}")
    return 0


if __name__ == "__main__": raise SystemExit(main())
