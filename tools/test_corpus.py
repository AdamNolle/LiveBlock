import hashlib
import http.client
import json
import threading
from http.server import ThreadingHTTPServer
from pathlib import Path

from PIL import Image
import pytest

from corpus.build_sports_corpus import HUMAN_REVIEW_ATTESTATION, build, split_for
from corpus.export_eval_fixtures import export as export_eval_fixtures
from corpus.fetch_wikimedia import fetch_category, fetch_query
from corpus.propose_labels import predict_yolo
from corpus.review_labels import (REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
                                  REQUIRED_PROMOTION_PLACEMENTS,
                                  REQUIRED_PROMOTION_PRESERVATION_KINDS, filter_queue,
                                  load_plan_details, load_plan_paths, review_plan,
                                  ReviewSession, handler_for, review_plan_status,
                                  review_queue, review_state_sha256, review_summary,
                                  save_rejection, save_review,
                                  validate_reviewer_identity)
from install_verified_model import install
from validate_coreml_detector import percentile
import verify_promotion
from verify_promotion import (artifact_sha256, compare_quality, validate_gate_limits,
                              validate_required_facets)


def make_item(root: Path, name: str, *, boxes: list[dict], group: str,
              license_name: str = "CC BY 4.0", review_method: str | None = None,
              preserve_regions: list[dict] | None = None):
    root.mkdir(parents=True, exist_ok=True)
    image_path = root / f"{name}.png"
    # Vary pixels by name so perceptual dedupe does not collapse every fixture.
    name_digest = hashlib.sha256(name.encode()).digest()
    color = tuple(16 + (value % 14) * 16 for value in name_digest[:3])
    Image.new("RGB", (64, 48), color).save(image_path)
    digest = hashlib.sha256(image_path.read_bytes()).hexdigest()
    record = {
        "license": license_name,
        "local_path": image_path.name,
        "sha256": digest,
        "source_page": f"https://example.test/{name}",
        "split_group": group,
    }
    with (root / "manifest.jsonl").open("a") as handle:
        handle.write(json.dumps(record) + "\n")
    labels = {"reviewed": True, "boxes": boxes}
    if review_method is not None:
        labels["review_method"] = review_method
    if review_method == "human":
        labels["review_attestation"] = HUMAN_REVIEW_ATTESTATION
        labels["reviewed_by"] = "test-reviewer"
        labels["reviewed_at"] = "2026-01-01T00:00:00+00:00"
    if preserve_regions is not None:
        labels["preserve_regions"] = preserve_regions
    image_path.with_suffix(".png.labels.json").write_text(json.dumps(labels))


def test_human_review_queue_and_atomic_approval(tmp_path):
    source = tmp_path / "source"
    make_item(source, "candidate", boxes=[], group="event",
              review_method="independent_visual_ai_review")
    queue = review_queue(source)
    assert [item["path"] for item in queue] == ["candidate.png"]
    assert queue[0]["priority"] > 0
    assert "AI boxes ready to verify" in queue[0]["priority_reasons"]
    box = {"class": "Logo", "x": 0.1, "y": 0.2, "width": 0.3, "height": 0.2,
           "placement": "jersey", "confidence": 0.9}
    keep = {"kind": "jersey_number", "x": 0.5, "y": 0.2, "width": 0.1, "height": 0.2}
    destination = save_review(source, "candidate.png", [box], [keep], "reviewer@example.test")
    saved = json.loads(destination.read_text())
    assert saved["review_method"] == "human"
    assert saved["reviewed"] is True
    assert saved["review_attestation"] == HUMAN_REVIEW_ATTESTATION
    assert saved["reviewed_by"] == "reviewer@example.test"
    assert saved["reviewed_at"].endswith("+00:00")
    assert "confidence" not in saved["boxes"][0]
    assert saved["preserve_regions"] == [keep]
    assert review_queue(source) == []
    summary = review_summary(source)
    assert summary["human_images"] == 1
    assert summary["placement_source_groups"]["jersey"]["human"] == 1
    assert summary["preservation_source_groups"]["jersey_number"]["human"] == 1


def test_reviewer_identity_is_required_printable_and_immutable():
    session = ReviewSession()
    with pytest.raises(ValueError, match="required"):
        session.require_reviewer()
    with pytest.raises(ValueError, match="printable"):
        validate_reviewer_identity("human\nreviewer")
    assert session.set_reviewer(" reviewer@example.test ") == "reviewer@example.test"
    assert session.require_reviewer() == "reviewer@example.test"
    assert session.set_reviewer("reviewer@example.test") == "reviewer@example.test"
    with pytest.raises(ValueError, match="fixed"):
        session.set_reviewer("someone-else")


def test_review_server_prompts_for_one_reviewer_and_reports_plan_progress(tmp_path):
    source = tmp_path / "source"
    make_item(source, "candidate", boxes=[], group="event",
              review_method="independent_visual_ai_review")
    session = ReviewSession()
    server = ThreadingHTTPServer(
        ("127.0.0.1", 0),
        handler_for(source, session, {"candidate.png"}, {"candidate.png": ["placement:jersey"]}),
    )
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    credentials = {"token": ""}

    def request(method, path, payload=None, *, include_token=True, origin=None):
        connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=2)
        body = json.dumps(payload).encode() if payload is not None else None
        headers = {}
        if body is not None:
            headers = {
                "Content-Type": "application/json",
                "Origin": origin or f"http://127.0.0.1:{server.server_port}",
            }
            if include_token and credentials["token"]:
                headers["X-LiveBlock-Review-Token"] = credentials["token"]
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        data = response.read()
        connection.close()
        return response.status, json.loads(data) if data.startswith((b"{", b"[")) else data.decode()

    try:
        status, payload = request("GET", "/api/session")
        assert status == 200 and payload["reviewer"] == ""
        assert payload["reviewToken"]
        credentials["token"] = payload["reviewToken"]
        status, queue = request("GET", "/api/items")
        assert status == 200 and len(queue) == 1
        revision = queue[0]["review_revision"]
        decision = {
            "path": "candidate.png", "review_revision": revision, "attested": True,
            "boxes": [], "preserve_regions": [], "negative_placements": ["jersey"],
        }
        status, message = request("POST", "/api/label", decision)
        assert status == 400 and "reviewer" in message
        status, message = request(
            "POST", "/api/session", {"reviewer": "human@example.test"},
            origin="https://attacker.example",
        )
        assert status == 400 and "Origin" in message
        status, message = request(
            "POST", "/api/session", {"reviewer": "human@example.test"}, include_token=False
        )
        assert status == 400 and "token" in message
        status, payload = request("POST", "/api/session", {"reviewer": "human@example.test"})
        assert status == 200 and payload == {"reviewer": "human@example.test"}
        status, message = request("POST", "/api/session", {"reviewer": "other@example.test"})
        assert status == 400 and "fixed" in message
        unattested = dict(decision, attested=False)
        status, message = request("POST", "/api/label", unattested)
        assert status == 400 and "attestation" in message
        status, payload = request("GET", "/api/status")
        assert status == 200 and payload["counts"] == {
            "approved": 0, "excluded": 0, "missing": 0, "pending": 1,
        }
        status, _payload = request("POST", "/api/label", decision)
        assert status == 200
        status, message = request("POST", "/api/label", decision)
        assert status == 400 and "no longer pending" in message
        status, payload = request("GET", "/api/status")
        assert status == 200 and payload["counts"]["approved"] == 1
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


def test_failed_review_replace_preserves_previous_sidecar_and_cleans_temp(tmp_path, monkeypatch):
    source = tmp_path / "source"
    make_item(source, "candidate", boxes=[], group="event",
              review_method="independent_visual_ai_review")
    sidecar = source / "candidate.png.labels.json"
    original = sidecar.read_bytes()

    def fail_replace(_source, _destination):
        raise OSError("injected replacement failure")

    monkeypatch.setattr("corpus.review_labels.os.replace", fail_replace)
    with pytest.raises(OSError, match="injected"):
        save_review(source, "candidate.png", [], reviewer="human")
    assert sidecar.read_bytes() == original
    assert list(source.glob(".candidate.png.labels.json.*.tmp")) == []


def test_review_queue_prioritizes_and_summarizes_contextual_hard_negatives(tmp_path):
    source = tmp_path / "source"
    make_item(source, "screen", boxes=[], group="screen-group",
              review_method="independent_visual_ai_review")
    labels_path = source / "screen.png.labels.json"
    labels = json.loads(labels_path.read_text())
    labels["negative_placements"] = ["ordinary_screen"]
    labels_path.write_text(json.dumps(labels))
    queue = review_queue(source)
    assert "hard negative ordinary_screen: 0/3 groups" in queue[0]["priority_reasons"]
    save_review(source, "screen.png", [], reviewer="reviewer",
                negative_placements=["ordinary_screen"])
    assert review_summary(source)["negative_placement_source_groups"]["ordinary_screen"] == {
        "all_review_methods": 1, "human": 1,
    }


def test_required_promotion_placements_include_broadcast_overlay():
    assert "broadcast_overlay" in REQUIRED_PROMOTION_PLACEMENTS
    with pytest.raises(ValueError, match="missing required facets"):
        validate_required_facets([], [], [])
    validate_required_facets(REQUIRED_PROMOTION_PLACEMENTS,
                             REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
                             REQUIRED_PROMOTION_PRESERVATION_KINDS)
    with pytest.raises(ValueError, match="weakens required limits"):
        validate_gate_limits(min_precision=0, min_recall=0, min_placement_recall=0,
                             max_false_positives=999, max_p95_ms=999)
    validate_gate_limits(min_precision=0.5, min_recall=0.5, min_placement_recall=0.5,
                         max_false_positives=10, max_p95_ms=10)


def test_review_plan_selects_independent_groups_and_reports_unfillable_facets(tmp_path):
    source = tmp_path / "source"
    make_item(source, "car", boxes=[{
        "class": "Logo", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2,
        "placement": "car_livery",
    }], group="car-group", review_method="independent_visual_ai_review")
    make_item(source, "jersey", boxes=[{
        "class": "Logo", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2,
        "placement": "jersey",
    }], group="jersey-group", review_method="independent_visual_ai_review")
    labels_path = source / "jersey.png.labels.json"
    labels = json.loads(labels_path.read_text())
    labels["negative_placements"] = ["ordinary_screen"]
    labels_path.write_text(json.dumps(labels))

    plan = review_plan(source)
    assert plan["selected_group_count"] == 2
    assert {item["source_group"] for item in plan["selected_groups"]} == {
        "car-group", "jersey-group",
    }
    assert plan["remaining_deficits"]["placement:broadcast_overlay"] == 3


def test_review_plan_file_limits_queue_to_selected_paths(tmp_path):
    plan_path = tmp_path / "plan.json"
    plan_path.write_text(json.dumps({"selected_groups": [
        {"path": "selected.png", "source_group": "a", "covers": ["placement:jersey"]},
        {"path": "already-reviewed.png", "source_group": "b", "covers": []},
    ]}))
    details = load_plan_details(plan_path)
    allowed = load_plan_paths(plan_path)
    queue = [{"path": "selected.png"}, {"path": "unplanned.png"}]
    assert allowed == {"selected.png", "already-reviewed.png"}
    assert filter_queue(queue, allowed, details) == [{
        "path": "selected.png", "plan_covers": ["placement:jersey"],
    }]
    assert filter_queue(queue, None) == queue

    plan_path.write_text(json.dumps({"selected_groups": [{"source_group": "missing"}]}))
    with pytest.raises(ValueError, match="non-empty path"):
        load_plan_paths(plan_path)
    plan_path.write_text(json.dumps({"selected_groups": [
        {"path": "selected.png", "covers": "not-a-list"},
    ]}))
    with pytest.raises(ValueError, match="covers must be a list"):
        load_plan_details(plan_path)


def test_review_plan_status_audits_approved_pending_excluded_and_missing(tmp_path):
    source = tmp_path / "source"
    make_item(source, "approved", boxes=[], group="approved-group")
    make_item(source, "excluded", boxes=[], group="excluded-group")
    make_item(source, "pending", boxes=[], group="pending-group")
    save_review(source, "approved.png", [], reviewer="reviewer")
    save_rejection(source, "excluded.png", "unusable", "reviewer")

    initial_fingerprint = review_state_sha256(source)
    plan_path = tmp_path / "plan.json"
    plan_path.write_text('{"selected_groups": []}')
    status = review_plan_status(source, {
        "approved.png", "excluded.png", "pending.png", "missing.png",
    }, plan_path)
    assert status["counts"] == {
        "approved": 1, "excluded": 1, "missing": 1, "pending": 1,
    }
    assert status["planned_group_count"] == 4
    assert status["review_state_sha256"] == initial_fingerprint
    assert len(status["plan_sha256"]) == 64
    assert {item["status"] for item in status["items"]} == {
        "approved", "excluded", "missing", "pending",
    }


def test_human_review_queue_counts_only_human_groups_and_defers_siblings(tmp_path):
    source = tmp_path / "source"
    jersey = {
        "class": "Logo", "x": 0.1, "y": 0.2, "width": 0.3, "height": 0.2,
        "placement": "jersey",
    }
    make_item(source, "approved", boxes=[jersey], group="approved-group",
              review_method="human")
    make_item(source, "approved-sibling", boxes=[jersey], group="approved-group",
              review_method="independent_visual_ai_review")
    make_item(source, "same-group-a", boxes=[jersey], group="same-group",
              review_method="independent_visual_ai_review")
    make_item(source, "same-group-b", boxes=[jersey], group="same-group",
              review_method="independent_visual_ai_review")
    make_item(source, "other-group", boxes=[jersey], group="other-group",
              review_method="independent_visual_ai_review")

    queue = review_queue(source, target_groups_per_placement=3)
    assert [item["path"] for item in queue] == [
        "other-group.png", "same-group-a.png", "same-group-b.png", "approved-sibling.png",
    ]
    assert "jersey: 1/3 groups" in queue[0]["priority_reasons"]
    assert queue[0]["group_position"] == 1
    assert queue[2]["group_position"] == 2
    assert "same split group; review after independent groups" in queue[2]["priority_reasons"]
    assert queue[3]["group_has_human_review"] is True
    assert "split group already has human approval" in queue[3]["priority_reasons"]


def test_reviewer_can_exclude_unusable_image_with_audit_reason(tmp_path):
    source = tmp_path / "source"
    make_item(source, "unusable", boxes=[], group="bad-group",
              review_method="independent_visual_ai_review")
    make_item(source, "usable", boxes=[], group="good-group",
              review_method="independent_visual_ai_review")
    destination = save_rejection(source, "unusable.png", "not actually a sports scene", "reviewer")
    rejected = json.loads(destination.read_text())
    assert rejected["excluded"] is True
    assert rejected["exclusion_reason"] == "not actually a sports scene"
    assert [item["path"] for item in review_queue(source)] == ["usable.png"]
    assert review_summary(source)["excluded_images"] == 1
    stats = build(source, tmp_path / "output", val_fraction=0, test_fraction=0)
    assert stats["skipped"]["reviewer_excluded"] == 1


def test_human_review_rejects_path_traversal(tmp_path):
    with pytest.raises(ValueError, match="invalid image path"):
        save_review(tmp_path, "../outside.jpg", [], reviewer="reviewer")


def test_promotion_comparison_rejects_quality_and_slice_regressions():
    baseline = {
        "precision": 0.6, "recall": 0.6, "fp": 2,
        "per_placement": {"jersey": {"recall": 0.7}},
        "negative_placements": {"ordinary_screen": {"false_positives": 0}},
        "preservation": {"team_crest": {"false_positives": 0}},
    }
    candidate = {
        "precision": 0.55, "recall": 0.65, "fp": 3,
        "per_placement": {"jersey": {"recall": 0.5}},
        "negative_placements": {"ordinary_screen": {"false_positives": 2}},
        "preservation": {"team_crest": {"false_positives": 1}},
    }
    failures = compare_quality(candidate, baseline, min_precision=0.5, min_recall=0.5,
                               min_placement_recall=0.5, placements=["jersey"],
                               max_false_positives=10)
    assert "precision regressed versus baseline" in failures
    assert "jersey recall regressed versus baseline" in failures
    assert "ordinary_screen hard-negative false positives must be zero" in failures
    assert "team_crest preservation false positives must be zero" in failures


def passing_promotion_report(candidate: Path) -> dict:
    artifact_keys = (
        "baseline_coreml", "baseline_model", "candidate_coreml", "candidate_model",
        "corpus", "fixtures", "pool",
    )
    return {
        "passed": True,
        "failures": [],
        "gate": {"schema": verify_promotion.GATE_SCHEMA,
                 "code_artifacts": verify_promotion.gate_code_artifacts()},
        "config": {
            **{key: str(candidate) for key in artifact_keys},
            "placement": list(REQUIRED_PROMOTION_PLACEMENTS),
            "negative_placement": list(REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS),
            "preservation_kind": list(REQUIRED_PROMOTION_PRESERVATION_KINDS),
            "min_precision": 0.5, "min_recall": 0.5,
            "min_placement_recall": 0.5, "max_false_positives": 10,
            "max_p95_ms": 10.0,
        },
        "corpus": {}, "fixtures": {}, "quality": {}, "coreml": {},
        "artifacts": {key: {
            "path": str(candidate), "sha256": artifact_sha256(candidate),
        } for key in artifact_keys},
    }


def test_verified_installer_requires_passing_fingerprint_and_preserves_backup(tmp_path):
    candidate = tmp_path / "candidate.mlpackage"
    candidate.mkdir()
    (candidate / "model.bin").write_bytes(b"new-model")
    destination = tmp_path / "liveblock-detector.mlpackage"
    destination.mkdir()
    (destination / "model.bin").write_bytes(b"old-model")
    report = tmp_path / "report.json"
    report.write_text(json.dumps(passing_promotion_report(candidate)))

    install(report, destination)
    assert (destination / "model.bin").read_bytes() == b"new-model"
    assert (tmp_path / "liveblock-detector.mlpackage.pre-promotion" / "model.bin").read_bytes() == b"old-model"


def test_verified_installer_rejects_changed_corpus_inputs(tmp_path):
    candidate = tmp_path / "candidate.mlpackage"
    candidate.mkdir()
    (candidate / "model.bin").write_bytes(b"model")
    payload = passing_promotion_report(candidate)
    payload["artifacts"]["pool"]["sha256"] = "0" * 64
    report = tmp_path / "report.json"
    report.write_text(json.dumps(payload))
    with pytest.raises(ValueError, match="pool fingerprint no longer matches"):
        install(report, tmp_path / "destination.mlpackage")


def test_verified_installer_rejects_failed_or_changed_candidate(tmp_path):
    candidate = tmp_path / "candidate.mlpackage"
    candidate.mkdir()
    (candidate / "model.bin").write_bytes(b"original")
    report = tmp_path / "report.json"
    report.write_text(json.dumps({"passed": False, "failures": ["quality failed"]}))
    with pytest.raises(ValueError, match="did not pass"):
        install(report, tmp_path / "destination.mlpackage")

    report.write_text(json.dumps(passing_promotion_report(candidate)))
    (candidate / "model.bin").write_bytes(b"changed")
    with pytest.raises(ValueError, match="no longer matches"):
        install(report, tmp_path / "destination.mlpackage")

    stale = json.loads(report.read_text())
    stale["gate"]["code_artifacts"]["eval/run_eval.py"] = "0" * 64
    report.write_text(json.dumps(stale))
    with pytest.raises(ValueError, match="gate code or dependencies changed"):
        install(report, tmp_path / "destination.mlpackage")


def test_latency_percentile_interpolates_sorted_values():
    assert percentile([40, 10, 30, 20], 0.5) == 25
    assert percentile([10, 20, 30], 0.95) == pytest.approx(29)


def test_group_split_is_deterministic_and_leak_free():
    assert split_for("same-event", 0.15, 0.15) == split_for("same-event", 0.15, 0.15)


def test_build_keeps_hard_negatives_and_runtime_classes(tmp_path):
    source = tmp_path / "source"
    box = {"class": "Logo", "x": 0.1, "y": 0.2, "width": 0.3, "height": 0.2,
           "placement": "jersey"}
    make_item(source, "positive", boxes=[box], group="event-a")
    make_item(source, "negative", boxes=[], group="event-b")

    output = tmp_path / "output"
    stats = build(source, output, val_fraction=0, test_fraction=0)
    assert stats["total_images"] == 2
    assert stats["classes"] == {"Logo": 1}
    assert stats["placements"] == {"jersey": 1}
    assert len(list((output / "images" / "train").glob("*"))) == 2
    assert any(not path.read_text() for path in (output / "labels" / "train").glob("*.txt"))
    assert "0: Logo" in (output / "data.yaml").read_text()


def test_export_eval_fixtures_uses_pixel_boxes(tmp_path):
    source = tmp_path / "source"
    box = {"class": "Ad banner", "x": 0.25, "y": 0.25, "width": 0.5, "height": 0.5,
           "placement": "venue_board"}
    make_item(source, "positive", boxes=[box], group="event-a")
    corpus = tmp_path / "corpus"
    build(source, corpus, val_fraction=0, test_fraction=0)
    fixtures = tmp_path / "fixtures"
    stats = export_eval_fixtures(corpus, fixtures, "train")
    assert stats == {
        "boxes": 1,
        "images": 1,
        "negatives": 0,
        "preserve_regions": 0,
        "split": "train",
    }
    sidecar_path = next(path for path in fixtures.glob("*.json") if path.name != "fixture-stats.json")
    sidecar = json.loads(sidecar_path.read_text())
    assert sidecar["boxes"][0]["class_id"] == 1
    assert sidecar["boxes"][0]["placement"] == "venue_board"
    assert sidecar["boxes"][0]["box"] == pytest.approx([16, 12, 32, 24])


def test_build_and_export_carry_explicit_jersey_preservation_regions(tmp_path):
    source = tmp_path / "source"
    keep = {
        "kind": "jersey_number", "x": 0.25, "y": 0.25, "width": 0.5, "height": 0.5,
    }
    make_item(source, "team-identity", boxes=[], group="event-a", preserve_regions=[keep])
    corpus = tmp_path / "corpus"
    stats = build(source, corpus, val_fraction=0, test_fraction=0)
    assert stats["preservation_regions"] == {"jersey_number": 1}
    annotation = json.loads((corpus / "annotations.jsonl").read_text().splitlines()[0])
    assert annotation["preserve_regions"] == [{
        "kind": "jersey_number", "box": [0.25, 0.25, 0.5, 0.5],
    }]
    fixtures = tmp_path / "fixtures"
    export_eval_fixtures(corpus, fixtures, "train")
    sidecar_path = next(path for path in fixtures.glob("*.json") if path.name != "fixture-stats.json")
    sidecar = json.loads(sidecar_path.read_text())
    assert sidecar["preserve_regions"] == [{
        "kind": "jersey_number", "box": [16, 12, 32, 24],
    }]


def test_build_and_export_preserve_hard_negative_placement_context(tmp_path):
    source = tmp_path / "source"
    make_item(source, "sports-screen", boxes=[], group="screen-event")
    labels_path = source / "sports-screen.png.labels.json"
    labels = json.loads(labels_path.read_text())
    labels["negative_placements"] = ["ordinary_screen", "broadcast_overlay", "ordinary_screen"]
    labels_path.write_text(json.dumps(labels))
    corpus = tmp_path / "corpus"
    build(source, corpus, val_fraction=0, test_fraction=0)
    annotation = json.loads((corpus / "annotations.jsonl").read_text().splitlines()[0])
    assert annotation["negative_placements"] == ["broadcast_overlay", "ordinary_screen"]
    fixtures = tmp_path / "fixtures"
    export_eval_fixtures(corpus, fixtures, "train")
    sidecar_path = next(path for path in fixtures.glob("*.json") if path.name != "fixture-stats.json")
    assert json.loads(sidecar_path.read_text())["negative_placements"] == [
        "broadcast_overlay", "ordinary_screen",
    ]


def test_build_rejects_sponsor_box_covering_preserved_identity(tmp_path):
    source = tmp_path / "source"
    box = {"class": "Logo", "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5,
           "placement": "car_livery"}
    keep = {"kind": "vehicle_number", "x": 0.2, "y": 0.2, "width": 0.2, "height": 0.2}
    make_item(source, "contradiction", boxes=[box], group="event", preserve_regions=[keep])
    with pytest.raises(ValueError, match="contradicts preserve region"):
        build(source, tmp_path / "output", val_fraction=0, test_fraction=0)


def test_build_rejects_unknown_positive_or_negative_placement(tmp_path):
    source = tmp_path / "source"
    make_item(source, "bad-positive", boxes=[{
        "class": "Logo", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2,
        "placement": "sideline-ish",
    }], group="event-positive")
    with pytest.raises(ValueError, match="unknown placement"):
        build(source, tmp_path / "positive-output", val_fraction=0, test_fraction=0)

    source = tmp_path / "negative-source"
    make_item(source, "bad-negative", boxes=[], group="event-negative")
    labels_path = source / "bad-negative.png.labels.json"
    labels = json.loads(labels_path.read_text())
    labels["negative_placements"] = ["somewhere"]
    labels_path.write_text(json.dumps(labels))
    with pytest.raises(ValueError, match="negative_placements must use"):
        build(source, tmp_path / "negative-output", val_fraction=0, test_fraction=0)


def test_build_rejects_unknown_preservation_kind(tmp_path):
    source = tmp_path / "source"
    make_item(source, "bad-keep", boxes=[], group="event", preserve_regions=[{
        "kind": "mascot", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2,
    }])
    with pytest.raises(ValueError, match="unknown preserve kind"):
        build(source, tmp_path / "output", val_fraction=0, test_fraction=0)


def test_build_rejects_unknown_class(tmp_path):
    source = tmp_path / "source"
    make_item(source, "bad", boxes=[{
        "class": "NASCAR", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2,
    }], group="event")
    with pytest.raises(ValueError, match="unknown class"):
        build(source, tmp_path / "output", val_fraction=0, test_fraction=0)


def test_build_rejects_human_review_without_reviewer_provenance(tmp_path):
    source = tmp_path / "source"
    make_item(source, "unattributed", boxes=[], group="event", review_method="human")
    labels_path = source / "unattributed.png.labels.json"
    labels = json.loads(labels_path.read_text())
    labels.pop("reviewed_by")
    labels.pop("reviewed_at")
    labels_path.write_text(json.dumps(labels))
    with pytest.raises(ValueError, match="no reviewed"):
        build(source, tmp_path / "output", val_fraction=0, test_fraction=0,
              review_methods={"human"})


def test_build_rejects_human_review_without_personal_attestation(tmp_path):
    source = tmp_path / "source"
    make_item(source, "unattested", boxes=[], group="event", review_method="human")
    labels_path = source / "unattested.png.labels.json"
    labels = json.loads(labels_path.read_text())
    labels.pop("review_attestation")
    labels_path.write_text(json.dumps(labels))
    with pytest.raises(ValueError, match="no reviewed"):
        build(source, tmp_path / "output", val_fraction=0, test_fraction=0,
              review_methods={"human"})
    assert review_queue(source)[0]["path"] == "unattested.png"


def test_build_can_require_explicit_human_review(tmp_path):
    source = tmp_path / "source"
    make_item(source, "human", boxes=[], group="event-a", review_method="human")
    make_item(source, "ai", boxes=[], group="event-b",
              review_method="independent_visual_ai_review")
    output = tmp_path / "output"
    stats = build(source, output, val_fraction=0, test_fraction=0, review_methods={"human"})
    assert stats["total_images"] == 1
    assert stats["review_methods"] == {"human": 1}
    assert stats["skipped"] == {"review_method": 1}


def test_build_stratifies_placement_groups_across_val_and_test(tmp_path):
    source = tmp_path / "source"
    for index, placement in enumerate(("jersey", "jersey", "jersey", "car_livery", "venue_board")):
        make_item(source, f"item-{index}", boxes=[{
            "class": "Logo", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2,
            "placement": placement,
        }], group=f"event-{index}")
    output = tmp_path / "output"
    stats = build(source, output, val_fraction=0.25, test_fraction=0.25,
                  stratify_placements={"jersey"})
    assert stats["placements_by_split"]["test"]["jersey"] == 1
    assert stats["placements_by_split"]["val"]["jersey"] == 1
    assert stats["placements_by_split"]["train"]["jersey"] == 1


def test_build_rejects_stratification_without_three_source_groups(tmp_path):
    source = tmp_path / "source"
    for index in range(2):
        make_item(source, f"jersey-{index}", boxes=[{
            "class": "Logo", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2,
            "placement": "jersey",
        }], group=f"event-{index}")
    make_item(source, "other", boxes=[], group="event-other")
    with pytest.raises(ValueError, match="needs 3 source groups"):
        build(source, tmp_path / "output", val_fraction=0.2, test_fraction=0.2,
              stratify_placements={"jersey"})


def test_build_stratifies_preservation_groups_across_all_splits(tmp_path):
    source = tmp_path / "source"
    keep = {"kind": "team_crest", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2}
    for index in range(5):
        make_item(source, f"identity-{index}", boxes=[], group=f"event-{index}",
                  preserve_regions=[keep] if index < 3 else [])
    output = tmp_path / "output"
    stats = build(source, output, val_fraction=0.2, test_fraction=0.2,
                  stratify_preservation_kinds={"team_crest"})
    for split in ("train", "val", "test"):
        assert stats["preservation_regions_by_split"][split]["team_crest"] == 1


def test_build_stratifies_contextual_hard_negatives_across_all_splits(tmp_path):
    source = tmp_path / "source"
    for index in range(5):
        make_item(source, f"screen-{index}", boxes=[], group=f"screen-{index}")
        if index < 3:
            labels_path = source / f"screen-{index}.png.labels.json"
            labels = json.loads(labels_path.read_text())
            labels["negative_placements"] = ["ordinary_screen"]
            labels_path.write_text(json.dumps(labels))
    stats = build(source, tmp_path / "output", val_fraction=0.2, test_fraction=0.2,
                  stratify_negative_placements={"ordinary_screen"})
    for split in ("train", "val", "test"):
        assert stats["negative_placements_by_split"][split]["ordinary_screen"] == 1


def test_build_requires_requested_test_placement_slice(tmp_path):
    source = tmp_path / "source"
    make_item(source, "only-car", boxes=[{
        "class": "Logo", "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2,
        "placement": "car_livery",
    }], group="event")
    with pytest.raises(ValueError, match="missing required placement slices"):
        build(source, tmp_path / "output", val_fraction=0, test_fraction=0,
              required_test_placements={"venue_board"})


def test_build_excludes_unapproved_license(tmp_path):
    source = tmp_path / "source"
    make_item(source, "restricted", boxes=[], group="event", license_name="all rights reserved")
    with pytest.raises(ValueError, match="no reviewed"):
        build(source, tmp_path / "output", val_fraction=0, test_fraction=0)


def test_fetch_category_uses_file_category_generator(tmp_path, monkeypatch):
    requests = []

    def fake_api(params):
        requests.append(params)
        return {"query": {"pages": []}}

    monkeypatch.setattr("corpus.fetch_wikimedia.api_json", fake_api)
    assert fetch_category("Match photos", tmp_path, 5, False) == 0
    assert requests[0]["generator"] == "categorymembers"
    assert requests[0]["gcmtitle"] == "Category:Match photos"
    assert requests[0]["gcmtype"] == "file"


def test_fetch_query_filters_irrelevant_titles_before_download(tmp_path, monkeypatch):
    pages = [
        {"title": "File:Unrelated interview.jpg", "imageinfo": [{
            "descriptionurl": "https://commons.example/unrelated", "url": "https://img.example/u.jpg",
            "extmetadata": {"LicenseShortName": {"value": "CC BY 4.0"}},
        }]},
        {"title": "File:Football match broadcast.jpg", "imageinfo": [{
            "descriptionurl": "https://commons.example/match", "url": "https://img.example/m.jpg",
            "extmetadata": {"LicenseShortName": {"value": "CC BY 4.0"}},
        }]},
    ]
    monkeypatch.setattr("corpus.fetch_wikimedia.api_json", lambda _params: {"query": {"pages": pages}})

    def fake_download(_url, destination):
        destination.write_bytes(b"image")
        return "digest"

    monkeypatch.setattr("corpus.fetch_wikimedia.download", fake_download)
    assert fetch_query("football television", tmp_path, 1, False,
                       request_delay=0, title_terms=("match", "scoreboard")) == 1
    manifest = [json.loads(line) for line in (tmp_path / "manifest.jsonl").read_text().splitlines()]
    assert [record["title"] for record in manifest] == ["File:Football match broadcast.jpg"]


def test_fetch_category_can_share_event_split_group(tmp_path, monkeypatch):
    captured = {}

    def fake_fetch(*args, **kwargs):
        captured.update(kwargs)
        return 0

    monkeypatch.setattr("corpus.fetch_wikimedia.fetch_source", fake_fetch)
    fetch_category("One match", tmp_path, 5, False, group_members=True)
    assert captured["shared_split_group"] == "wikimedia-category:One match"


def test_predict_yolo_normalizes_top_left_boxes():
    class Scalar:
        def __init__(self, value):
            self.value = value

        def item(self):
            return self.value

    class Coordinates:
        def __getitem__(self, _index):
            return self

        def tolist(self):
            return [20, 10, 60, 30]

    class Box:
        cls = [Scalar(0)]
        conf = [Scalar(0.75)]
        xyxy = Coordinates()

    class Result:
        orig_shape = (100, 200)
        boxes = [Box()]

    class Model:
        def __call__(self, *_args, **_kwargs):
            return [Result()]

    assert predict_yolo(Model(), Path("sample.jpg"), 0.1) == [{
        "class": "Logo",
        "confidence": 0.75,
        "height": 0.2,
        "placement": "unknown",
        "width": 0.2,
        "x": 0.1,
        "y": 0.1,
    }]
