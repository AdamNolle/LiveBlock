"""Unit tests for build_openvocab pure logic (no model download / torch)."""
import pytest

from build_openvocab import load_vocab


def test_load_vocab_primary_prompt_and_names(tmp_path):
    p = tmp_path / "v.json"
    p.write_text(
        '{"version":1,"classes":['
        '{"id":1,"name":"Ad","prompts":["advertising banner","advertisement"]},'
        '{"id":0,"name":"Logo","prompts":["brand logo","logo"]}]}'
    )
    prompts, names = load_vocab(str(p))
    # One prompt per class, sorted by id so index == class id.
    assert prompts == ["brand logo", "advertising banner"]
    assert names == {0: "Logo", 1: "Ad"}


@pytest.mark.parametrize("classes", [
    [],
    [{"id": 1, "name": "Ad", "prompts": ["ad"]}],
    [{"id": 0, "name": "Ad", "prompts": ["ad"]},
     {"id": 0, "name": "Logo", "prompts": ["logo"]}],
    [{"id": 0, "name": "", "prompts": ["ad"]}],
    [{"id": 0, "name": "Ad", "prompts": []}],
])
def test_load_vocab_rejects_unsafe_schema(tmp_path, classes):
    import json
    p = tmp_path / "invalid.json"
    p.write_text(json.dumps({"version": 1, "classes": classes}))
    with pytest.raises((ValueError, KeyError)):
        load_vocab(str(p))


def test_load_vocab_reads_checked_in_default():
    from pathlib import Path
    default = Path(__file__).resolve().parent / "vocab" / "liveblock-vocab.json"
    prompts, names = load_vocab(str(default))
    # 3 classes → 3 primary prompts; the Ad-banner primary is the strong one.
    assert len(prompts) == 3
    assert prompts[0] == "brand logo"
    assert prompts[1] == "advertising banner"
    assert names == {0: "Logo", 1: "Ad banner", 2: "Sponsored"}

    bundled = Path(__file__).resolve().parents[1] / "Sources" / "Resources" / "liveblock-vocab.json"
    assert load_vocab(str(bundled)) == (prompts, names)
