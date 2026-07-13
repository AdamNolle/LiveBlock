"""Lightweight immutable schema-5 policy shared by verification and signing."""
from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

REQUIRED_PROMOTION_PLACEMENTS = (
    "car_livery", "jersey", "venue_board", "ordinary_screen", "broadcast_overlay",
)
REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS = (
    "ordinary_screen", "broadcast_overlay", "jersey",
)
REQUIRED_PROMOTION_PRESERVATION_KINDS = (
    "team_name", "jersey_number", "vehicle_number", "team_crest", "manufacturer_badge",
)

GATE_SCHEMA = 5
REQUIRED_MIN_PRECISION = 0.50
REQUIRED_MIN_RECALL = 0.50
REQUIRED_MIN_PLACEMENT_RECALL = 0.50
REQUIRED_MAX_FALSE_POSITIVES = 10
REQUIRED_MAX_P95_MS = 10.0
DEFAULT_MODEL_SIGNING_KEY_ENV = "LIVEBLOCK_MODEL_SIGNING_KEY_B64"


def _file_sha256_bytes(path: Path) -> bytes:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.digest()


def artifact_sha256(path: Path) -> str:
    """Implement the cross-platform ``sha256-file-or-tree-v1`` contract."""
    if path.is_symlink():
        raise ValueError(f"artifact symlink is forbidden: {path}")
    if path.is_file():
        return _file_sha256_bytes(path).hex()
    if not path.is_dir():
        raise FileNotFoundError(path)

    digest = hashlib.sha256()
    digest.update(b"liveblock-tree-sha256-v1\0")
    files: list[tuple[bytes, Path]] = []
    for item in path.rglob("*"):
        if item.is_symlink():
            raise ValueError(f"artifact symlink is forbidden: {item}")
        if item.is_dir():
            continue
        if not item.is_file():
            raise ValueError(f"unsupported artifact entry: {item}")
        relative = item.relative_to(path).as_posix().encode("utf-8")
        files.append((relative, item))
    for relative, item in sorted(files, key=lambda entry: entry[0]):
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(_file_sha256_bytes(item))
    return digest.hexdigest()


def gate_code_artifacts() -> dict[str, str]:
    root = Path(__file__).resolve().parent
    paths = {
        "promotion_contract.py": root / "promotion_contract.py",
        "verify_promotion.py": root / "verify_promotion.py",
        "corpus/build_sports_corpus.py": root / "corpus" / "build_sports_corpus.py",
        "corpus/export_eval_fixtures.py": root / "corpus" / "export_eval_fixtures.py",
        "eval/run_eval.py": root / "eval" / "run_eval.py",
        "validate_coreml_detector.py": root / "validate_coreml_detector.py",
    }
    return {name: artifact_sha256(path) for name, path in paths.items()}


def promotion_command(recipe: dict, output: Path) -> list[str]:
    """Reconstruct the immutable gate command from a prior report's recipe."""
    config = recipe.get("config")
    if not isinstance(config, dict):
        raise ValueError("promotion recipe lacks a config object")
    command = [sys.executable, str(Path(__file__).with_name("verify_promotion.py"))]
    scalar_options = (
        "pool", "corpus", "fixtures", "candidate_model", "baseline_model",
        "candidate_coreml", "baseline_coreml", "min_precision", "min_recall",
        "min_placement_recall", "max_false_positives", "max_p95_ms", "score",
        "benchmark_runs",
    )
    for name in scalar_options:
        value = config.get(name)
        if value is None or isinstance(value, bool):
            raise ValueError(f"promotion recipe lacks valid {name}")
        command.extend((f"--{name.replace('_', '-')}", str(value)))
    for name in ("placement", "negative_placement", "preservation_kind"):
        values = config.get(name)
        if not isinstance(values, list) or not values:
            raise ValueError(f"promotion recipe lacks valid {name}")
        for value in values:
            if not isinstance(value, str) or not value:
                raise ValueError(f"promotion recipe has invalid {name}")
            command.extend((f"--{name.replace('_', '-')}", value))
    command.extend(("--output", str(output), "--exclusive-output"))
    return command


def rerun_promotion_gate(
    recipe_path: Path,
    output: Path,
    secret_environment_names: tuple[str, ...] = (),
) -> None:
    """Run the current gate and preserve its report at a create-new path."""
    if output.exists() or output.is_symlink():
        raise FileExistsError(f"attested promotion output already exists: {output}")
    recipe = json.loads(recipe_path.read_bytes())
    output.parent.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    for name in secret_environment_names:
        environment.pop(name, None)
    result = subprocess.run(
        promotion_command(recipe, output),
        cwd=Path(__file__).resolve().parents[1],
        env=environment,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip().splitlines()
        suffix = f": {detail[-1]}" if detail else ""
        raise ValueError(
            f"fresh schema-5 promotion rerun failed; inspect {output}{suffix}"
        )
    if output.is_symlink() or not output.is_file():
        raise ValueError("promotion gate reported success without a regular report file")


def validate_required_facets(placements, negative_placements, preservation_kinds) -> None:
    requested = {
        "placement": set(placements),
        "negative-placement": set(negative_placements),
        "preservation-kind": set(preservation_kinds),
    }
    required = {
        "placement": set(REQUIRED_PROMOTION_PLACEMENTS),
        "negative-placement": set(REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS),
        "preservation-kind": set(REQUIRED_PROMOTION_PRESERVATION_KINDS),
    }
    missing = {
        kind: sorted(values - requested[kind])
        for kind, values in required.items() if values - requested[kind]
    }
    if missing:
        raise ValueError(f"promotion configuration missing required facets: {missing}")


def validate_gate_limits(*, min_precision: float, min_recall: float,
                         min_placement_recall: float, max_false_positives: int,
                         max_p95_ms: float) -> None:
    failures = []
    if min_precision < REQUIRED_MIN_PRECISION:
        failures.append(f"min_precision must be >= {REQUIRED_MIN_PRECISION}")
    if min_recall < REQUIRED_MIN_RECALL:
        failures.append(f"min_recall must be >= {REQUIRED_MIN_RECALL}")
    if min_placement_recall < REQUIRED_MIN_PLACEMENT_RECALL:
        failures.append(f"min_placement_recall must be >= {REQUIRED_MIN_PLACEMENT_RECALL}")
    if max_false_positives > REQUIRED_MAX_FALSE_POSITIVES:
        failures.append(f"max_false_positives must be <= {REQUIRED_MAX_FALSE_POSITIVES}")
    if max_p95_ms > REQUIRED_MAX_P95_MS:
        failures.append(f"max_p95_ms must be <= {REQUIRED_MAX_P95_MS}")
    if failures:
        raise ValueError("promotion configuration weakens required limits: " + "; ".join(failures))
