"""Unit tests for build_openvocab pure logic (no model download / torch)."""
from build_openvocab import load_vocab


def test_load_vocab_flattens_prompts(tmp_path):
    p = tmp_path / "v.json"
    p.write_text(
        '{"version":1,"classes":['
        '{"id":0,"name":"Logo","prompts":["logo","brand logo"]},'
        '{"id":1,"name":"Ad","prompts":["advertisement"]}]}'
    )
    assert load_vocab(str(p)) == ["logo", "brand logo", "advertisement"]


def test_load_vocab_reads_checked_in_default():
    from pathlib import Path
    default = Path(__file__).resolve().parent / "vocab" / "liveblock-vocab.json"
    prompts = load_vocab(str(default))
    # 3 classes x 3 prompts each in the shipped default vocabulary.
    assert len(prompts) == 9
    assert prompts[0] == "logo"
