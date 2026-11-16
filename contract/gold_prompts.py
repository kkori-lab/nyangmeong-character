"""프롬프트·눈 판독 정답 만들기 : 원본 Python(`nyangmeong-ai` `app/character/engine.py`)이 낸 문자열을 쓴다

러스트 테스트(`tests/prompts_gold.rs`)가 이 정답과 문자열이 같은지 비교한다.
원본 가상환경의 Python으로 부른다(`-I`: 현재 폴더의 파이썬 파일을 불러오지 않는다).

사용: <ai python> -I contract/gold_prompts.py <nyangmeong-ai 경로> <출력 json>
"""

import json
import sys
from pathlib import Path

ai = Path(sys.argv[1]).resolve()
sys.path.insert(0, str(ai))

from app.character import engine  # noqa: E402

REGNS = ["MYPC0001", "MYPC0002", "MYPC0003", "MYPC0004", "MYPC0005"]
REGISTERED = [
    (None, None, None),
    ("말티즈", None, None),
    (None, "흰색", "겁이 많음"),
    ('믹스 "점박이"', "갈색 {eyes} {hint}", "줄바꿈\n탭\t역슬래시\\"),
    ("", "", ""),
    (" ", "😀", "  \x7f \x01"),
]
HINTS = [
    "a red bandana",
    "a dog-shaped hat for a dog",
    "{eyes} {{braces}} {species}",
    "",
    None,
]
ANSWERS = [
    "",
    "   ",
    "no json here",
    '{"shape": "round", "color": "blue"}',
    'Here: {"shape": "Almond", "color": "Dark Brown"} done',
    '```json\n{"shape": "upturned", "color": "odd"}\n```',
    '{"shape": "square", "color": "pink"}',
    '{"shape": 1, "color": ["blue"]}',
    '{"shape": null, "color": ""}',
    '[{"shape": "droopy"}]',
    '{"shape": "droopy"} and {"color": "green"}',
    '} {"shape": "droopy"',
    '{"shape": "droopy", "shape": "almond"}',
    '{"shape": "ALMOND ", "color": "\x1fgreen"}',
    '{"shape": "dark brown"}',
    "{'shape': 'round'}",
    '{"shape": NaN}',
    '{"shape": NaN, "color": "blue"}',
]
HINT_ANSWERS = [
    "",
    "\n\n",
    "a red cap",
    "- a red cap.",
    '* "a red cap".',
    "-  ` a red cap `  ..",
    "first line\nsecond line",
    "\n\n  third\r\nfourth",
    "x y",
    "x\x85y",
    "  \x1f padded \x1f ",
    "a" * 150,
    "a" * 151,
    "가" * 150,
    "가" * 151,
    "...",
    '"',
    "- - * ",
    "a.b.",
]

out: dict = {"prompts": {}}

eyes = []
for species in ("dog", "cat"):
    for shape in engine.EYE_SHAPES:
        for color in (*engine.EYE_COLORS, None):
            for reg in REGISTERED:
                eyes.append(
                    {
                        "species": species,
                        "shape": shape,
                        "color": color,
                        "vrty": reg[0],
                        "colr": reg[1],
                        "chrc": reg[2],
                        "prompt": engine.build_prompt(species, engine.Eyes(shape, color), *reg),
                    }
                )
out["build_prompt"] = eyes
out["eye_question"] = {s: engine.eye_question(s) for s in ("dog", "cat")}

wear = []
for species in ("dog", "cat"):
    for regn in [*REGNS, "MYPC0006", "X"]:
        for hint in HINTS:
            for by_image in (False, True):
                item = engine.BakeItem(1, regn, hint, by_image)
                wear.append(
                    {
                        "species": species,
                        "regn": regn,
                        "hint": hint,
                        "by_image": by_image,
                        "prompt": engine.build_wear_prompt(species, item, by_image),
                    }
                )
out["build_wear_prompt"] = wear

out["parse_eyes"] = [
    {
        "answer": a,
        "shape": (e := engine.parse_eyes(a)).shape,
        "color": e.color,
    }
    for a in ANSWERS
]
out["parse_bake_hint"] = [{"answer": a, "hint": engine.parse_bake_hint(a)} for a in HINT_ANSWERS]

out["item_texts"] = [
    {
        "regn": regn,
        "name": name,
        "hint": engine.item_hint_question(regn, name),
        "pixel": engine.item_pixel_prompt(regn, name),
    }
    for regn in REGNS
    for name in ("모자", "{slot} {item_nm}", 'a "b"\n')
]
out["species"] = {"PETC0001": engine.species_of("PETC0001"), "PETC0002": engine.species_of("PETC0002")}
out["style_ref_bytes"] = len(engine.STYLE_REF.read_bytes())

Path(sys.argv[2]).write_text(json.dumps(out, ensure_ascii=False), encoding="utf-8")
