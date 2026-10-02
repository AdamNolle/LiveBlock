#!/usr/bin/env python3
"""Package an already-built x64 Windows executable with its runtime resources."""
import argparse
import hashlib
import json
import struct
import tempfile
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

ROOT = Path(__file__).resolve().parents[1]


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require_x64(path: Path) -> None:
    data = path.read_bytes()
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError(f"Not a PE executable: {path}")
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset + 6 > len(data) or data[offset:offset + 4] != b"PE\0\0" or struct.unpack_from("<H", data, offset + 4)[0] != 0x8664:
        raise ValueError(f"Expected AMD64 PE: {path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "platform/windows/target/x86_64-pc-windows-gnu/release")
    parser.add_argument("--output", type=Path, default=Path.home() / "Library/Caches/LiveBlock/release/LiveBlock-windows-x64-portable.zip")
    args = parser.parse_args()
    resources = ROOT / "platform/windows/src-tauri/resources"
    names = ("onnxruntime.dll", "onnxruntime_providers_shared.dll", "onnxruntime-LICENSE", "onnxruntime-ThirdPartyNotices.txt", "onnxruntime-build.json", "yolov8n.onnx", "yolov8n.classes.json")
    files = {"LiveBlock.exe": args.build_dir / "liveblock-windows.exe", "WebView2Loader.dll": args.build_dir / "WebView2Loader.dll"}
    files.update({f"resources/{name}": resources / name for name in names})
    files.update({"resources/MODEL_CARD.md": ROOT / "MODEL_CARD.md", "resources/MODEL_ATTRIBUTION.md": ROOT / "models/ATTRIBUTION.md"})
    for path in files.values():
        if not path.is_file():
            raise FileNotFoundError(path)
    frontend = ROOT / "platform/_shared-frontend/dist"
    assets = sorted(path for path in frontend.rglob("*") if path.is_file())
    if not assets or max(path.stat().st_mtime_ns for path in assets) > files["LiveBlock.exe"].stat().st_mtime_ns:
        raise ValueError("Build the Windows executable after building the current frontend")
    executable = files["LiveBlock.exe"].read_bytes()
    missing_assets = [str(path.relative_to(frontend)) for path in assets if str(path.relative_to(frontend)).encode() not in executable]
    if missing_assets:
        raise ValueError("Windows executable is missing current embedded UI assets. Build with --features custom-protocol: " + ", ".join(missing_assets))
    for name in ("LiveBlock.exe", "WebView2Loader.dll", "resources/onnxruntime.dll", "resources/onnxruntime_providers_shared.dll"):
        require_x64(files[name])
    metadata = json.loads((resources / "yolov8n.classes.json").read_text())
    if metadata.get("model_sha256") != digest(resources / "yolov8n.onnx"):
        raise ValueError("Model class metadata does not match the packaged weights")
    readme = """LiveBlock Windows x64 portable build

Extract the entire folder, then launch LiveBlock.exe. Keep WebView2Loader.dll
beside the executable and the resources folder intact.

Requires Windows 10 version 1903 or newer, the Microsoft Edge WebView2 Runtime,
and the Microsoft Visual C++ Redistributable for x64 (v14 runtime).
Use Microsoft's official installers if these dependencies are missing:
https://developer.microsoft.com/microsoft-edge/webview2/
https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist

This executable was cross-built on macOS. It is unsigned and has not yet passed
native Windows capture, overlay placement, or DirectML runtime tests.
Automatic detection starts off. Read resources/MODEL_CARD.md before enabling it.
Choose "Cover only · no capture" for opaque manual covers that do not start
screen capture. Automatic detection and screenshots require "Screen blend".
DRM-protected pixels may remain black or unavailable; capture is not guaranteed
to be undetectable. Apple notarization applies to the separate macOS app.
"""
    manifest = {"architecture": "x86_64", "native_runtime_verified": False, "frontend_assets": {str(path.relative_to(frontend)): digest(path) for path in assets}, "files": {name: digest(path) for name, path in sorted(files.items())}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=args.output.parent, suffix=".zip", delete=False) as temporary:
        staged = Path(temporary.name)
    try:
        with ZipFile(staged, "w", ZIP_DEFLATED) as archive:
            for name, path in sorted(files.items()):
                archive.write(path, f"LiveBlock/{name}")
            archive.writestr("LiveBlock/README.txt", readme)
            archive.writestr("LiveBlock/manifest.json", json.dumps(manifest, indent=2) + "\n")
        with ZipFile(staged) as archive:
            if bad := archive.testzip():
                raise ValueError(f"Archive CRC check failed: {bad}")
        staged.replace(args.output)
    finally:
        staged.unlink(missing_ok=True)
    args.output.with_suffix(".zip.sha256").write_text(f"{digest(args.output)}  {args.output.name}\n")
    print(f"Verified portable archive: {args.output}")


if __name__ == "__main__":
    main()
