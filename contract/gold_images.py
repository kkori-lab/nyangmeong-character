"""이미지 계산 정답 만들기 : 원본 Python(`nyangmeong-ai` `app/character/`)으로 정답 PNG·값을 쓴다

러스트 테스트(`tests/image_gold.rs`·`tests/ndimage_gold.rs`)가 이 정답과 픽셀 단위로 비교한다.
원본 가상환경의 Python으로 부른다(`-I`: 현재 폴더의 파이썬 파일을 불러오지 않는다).

사용:
  <ai python> -I contract/gold_images.py synth <nyangmeong-ai 경로> <tests/fixtures/image>
      가짜 그림(실제 반려동물 아님)과 정답을 만든다. 저장소에 커밋한다
  <ai python> -I contract/gold_images.py real <nyangmeong-ai 경로> <시트 폴더> <정답 폴더>
      시트 폴더(bare.png, worn_<부위코드>.png)로 정답을 만든다. 커밋하지 않는다
"""

import io
import json
import struct
import sys
import zlib
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage

SS = 4  # 그릴 때 4배로 그려 줄임 : 가장자리가 배경과 섞임(털 가장자리 처리 시험)
MAGENTA = (255, 0, 255)
REGNS = ("MYPC0001", "MYPC0002", "MYPC0003", "MYPC0004", "MYPC0005")


def fl(value) -> str:
    """float는 repr 문자열로 남긴다 : serde_json의 기본 실수 읽기는 마지막 한 비트가 틀릴 수 있다"""
    return repr(float(value))


def dump(path: Path, value) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=None), encoding="utf-8")


# ---------- 가짜 시트 그리기 ----------


class Painter:
    def __init__(self, width: int, background, alpha: bool):
        self.u = width / 1024
        self.width = width
        fill = (0, 0, 0, 0) if alpha else (*background, 255)
        self.image = Image.new("RGBA", (width * SS, width * SS), fill)
        self.d = ImageDraw.Draw(self.image)
        self.alpha = alpha

    def p(self, value: float) -> int:
        return int(round(value * self.u * SS))

    def ellipse(self, cx, cy, rx, ry, fill, line=None, stroke=3):
        self.d.ellipse(
            [self.p(cx - rx), self.p(cy - ry), self.p(cx + rx), self.p(cy + ry)],
            fill=fill,
            outline=line,
            width=self.p(stroke) if line else 0,
        )

    def rect(self, x0, y0, x1, y1, fill, line=None, stroke=3):
        self.d.rectangle(
            [self.p(x0), self.p(y0), self.p(x1), self.p(y1)],
            fill=fill,
            outline=line,
            width=self.p(stroke) if line else 0,
        )

    def polygon(self, points, fill, line=None):
        pts = [(self.p(x), self.p(y)) for x, y in points]
        self.d.polygon(pts, fill=fill, outline=line)

    def stroke(self, points, fill, width):
        pts = [(self.p(x), self.p(y)) for x, y in points]
        self.d.line(pts, fill=fill, width=self.p(width))

    def finish(self) -> Image.Image:
        small = self.image.resize((self.width, self.width), Image.BOX)
        return small if self.alpha else small.convert("RGB")


def pet(pt: Painter, ox, oy, fur, line, eyes, wide=1.0, dx=0, dy=0, holes=False):
    """칸 왼쪽 위 (ox, oy)에 그림. dx·dy는 그림만 옮김(착용 시트가 조금 밀려 그려지는 경우)"""
    cx = ox + 256 + dx
    cy = oy + dy
    pt.ellipse(cx + 150 * wide, cy + 320, 30, 60, fur, line)  # 꼬리
    for x in (-85, 45):
        pt.rect(cx + x * wide, cy + 380, cx + (x + 40) * wide, cy + 450, fur, line)  # 다리
    pt.ellipse(cx, cy + 330, 130 * wide, 95, fur, line)  # 몸
    pt.polygon([(cx - 76, cy + 130), (cx - 56, cy + 70), (cx - 26, cy + 115)], fur, line)
    pt.polygon([(cx + 76, cy + 130), (cx + 56, cy + 70), (cx + 26, cy + 115)], fur, line)
    pt.ellipse(cx, cy + 190, 95, 85, fur, line)  # 머리
    if eyes != "none":
        for x in (-41, 41):
            pt.ellipse(cx + x, cy + 180, 10, 10, (8, 8, 8))
            if eyes == "highlight":
                pt.ellipse(cx + x - 3, cy + 177, 5, 5, (255, 255, 255))
        pt.ellipse(cx, cy + 215, 8, 6, (20, 10, 10))  # 코
    if holes:
        pt.ellipse(cx - 40, cy + 340, 4, 4, MAGENTA)
        pt.ellipse(cx + 60, cy + 340, 40, 40, MAGENTA)


def item_on(pt: Painter, regn: str, ox, oy, dx=0, dy=0):
    cx = ox + 256 + dx
    cy = oy + dy
    ink = (20, 20, 20)
    if regn == "MYPC0001":  # 머리 : 모자
        pt.polygon([(cx - 66, cy + 130), (cx, cy + 20), (cx + 66, cy + 130)], (200, 40, 40), ink)
    elif regn == "MYPC0002":  # 얼굴 : 안경
        for x in (-41, 41):
            pt.ellipse(cx + x, cy + 180, 24, 24, None, ink, 5)
        pt.stroke([(cx - 17, cy + 180), (cx + 17, cy + 180)], ink, 5)
    elif regn == "MYPC0003":  # 목 : 목도리
        pt.rect(cx - 91, cy + 262, cx + 91, cy + 290, (40, 70, 200), ink)
    elif regn == "MYPC0004":  # 의상 : 조끼
        pt.rect(cx - 106, cy + 290, cx + 106, cy + 380, (40, 160, 70), ink)
    elif regn == "MYPC0005":  # 가방 : 상자와 끈
        pt.rect(cx + 84, cy + 310, cx + 144, cy + 380, (130, 50, 170), ink)
        pt.stroke([(cx + 44, cy + 250), (cx + 114, cy + 310)], (110, 70, 30), 8)


def sheet(width, fur, line, eyes, alpha=False, wide=1.0, holes=False, shift=(0, 0), regn=None, count=4):
    """2×2 시트: 윗줄 앉기, 아랫줄 서기 / 왼쪽 정면, 오른쪽 옆모습. 서기 정면(왼쪽 아래)만 착용·밀림 적용"""
    pt = Painter(width, MAGENTA, alpha)
    cells = [(0, 0), (512, 0), (0, 512), (512, 512)][:count]
    for index, (ox, oy) in enumerate(cells):
        front = index == 2
        pet(
            pt,
            ox,
            oy,
            fur,
            line,
            eyes if front else "dark",
            wide=wide if front else 1.0,
            dx=shift[0] if front else 0,
            dy=shift[1] if front else 0,
            holes=holes and front,
        )
        if front and regn:
            item_on(pt, regn, ox, oy, *shift)
    return pt.finish()


def png_bytes(image: Image.Image) -> bytes:
    buffer = io.BytesIO()
    image.save(buffer, "PNG", optimize=True)
    return buffer.getvalue()


SHIFTS = {
    "MYPC0001": (3, -2),
    "MYPC0002": (-4, 3),
    "MYPC0003": (6, 0),
    "MYPC0004": (0, -5),
    "MYPC0005": (2, 2),
}
TAN = (205, 165, 120)
CHARCOAL = (48, 48, 52)
LINE = (20, 20, 20)


def make_cases() -> dict[str, dict[str, bytes]]:
    """사례 이름 → {파일 이름: PNG 바이트}"""
    cases: dict[str, dict[str, bytes]] = {}

    def add(name, bare, worn=None):
        files = {"bare.png": png_bytes(bare)}
        for regn, image in (worn or {}).items():
            files[f"worn_{regn}.png"] = png_bytes(image)
        cases[name] = files

    def worn_set(regns, **kw):
        return {r: sheet(regn=r, shift=SHIFTS[r], **kw) for r in regns}

    plain = dict(width=1024, fur=TAN, line=LINE, eyes="dark")
    add("plain", sheet(**plain), worn_set(REGNS, **plain))
    dark = dict(width=1024, fur=CHARCOAL, line=(0, 0, 0), eyes="highlight")
    add("dark", sheet(**dark), worn_set(REGNS, **dark))
    clear = dict(width=1024, fur=TAN, line=LINE, eyes="dark", alpha=True)
    add("alpha", sheet(**clear), worn_set(("MYPC0001", "MYPC0002", "MYPC0004"), **clear))
    big = dict(width=1536, fur=TAN, line=LINE, eyes="dark")
    add("big1536", sheet(**big), worn_set(("MYPC0001", "MYPC0004"), **big))
    holes = dict(width=1024, fur=TAN, line=LINE, eyes="dark", holes=True)
    add("holes", sheet(**holes), worn_set(("MYPC0004",), **holes))
    wide = dict(width=1024, fur=TAN, line=LINE, eyes="dark", wide=2.0)
    add("wide", sheet(**wide), worn_set(("MYPC0004",), **wide))
    nothing = dict(width=1024, fur=TAN, line=LINE, eyes="none")
    add("noeyes", sheet(**nothing), worn_set(("MYPC0001",), **nothing))
    same = sheet(**plain)
    add("same_worn", same, {"MYPC0004": same})
    add("err_three", sheet(**plain, count=3))
    # 같은 칸(왼쪽 위)에 덩어리 둘, 오른쪽 아래 칸은 빔
    layout = Painter(1024, MAGENTA, False)
    for ox, oy in [(0, 0), (512, 0), (0, 512)]:
        pet(layout, ox, oy, TAN, LINE, "dark")
    layout.ellipse(475, 90, 25, 25, (30, 160, 40))
    add("err_layout", layout.finish())
    return cases


def make_items() -> dict[str, bytes]:
    items: dict[str, bytes] = {}

    def canvas(size, background, alpha=False):
        pt = Painter(size, background, alpha)
        return pt

    # 모자 : 마젠타 배경, 점 먼지
    pt = canvas(512, MAGENTA)
    pt.polygon([(150, 700), (512, 150), (874, 700)], (200, 40, 40), (20, 20, 20))
    pt.rect(130, 690, 894, 760, (240, 220, 60), (20, 20, 20))
    for x, y in [(30, 40), (900, 60), (60, 950), (950, 940)]:
        pt.rect(x, y, x + 6, y + 6, (50, 50, 50))
    items["hat"] = png_bytes(pt.finish())
    # 안경 : 안쪽이 배경색인 구멍
    pt = canvas(512, MAGENTA)
    for x in (320, 704):
        pt.ellipse(x, 512, 150, 130, None, (20, 20, 20), 28)
    pt.stroke([(470, 512), (554, 512)], (20, 20, 20), 28)
    items["glasses"] = png_bytes(pt.finish())
    # 투명 배경
    pt = canvas(512, MAGENTA, alpha=True)
    pt.ellipse(512, 512, 300, 200, (40, 160, 70), (20, 20, 20), 12)
    items["alpha"] = png_bytes(pt.finish())
    # 흰 배경 : 채도가 낮아 가장자리 처리 없음
    pt = canvas(400, (255, 255, 255))
    pt.rect(300, 200, 700, 800, (60, 80, 200), (20, 20, 20), 10)
    items["white"] = png_bytes(pt.finish())
    # 비어 있음 : 점 먼지뿐
    pt = canvas(256, MAGENTA)
    pt.rect(100, 100, 104, 104, (10, 10, 10))
    items["empty"] = png_bytes(pt.finish())
    # 1024보다 큰 정사각 : NEAREST로 줄이는 길
    pt = canvas(1100, MAGENTA)
    pt.polygon([(200, 800), (512, 200), (824, 800)], (200, 40, 40), (20, 20, 20))
    items["big"] = png_bytes(pt.finish())
    return items


def png16_bytes(array: np.ndarray, color_type: int) -> bytes:
    """16비트 PNG(Pillow는 쓰지 못하므로 직접 씀) : color_type 0 회색, 2 RGB, 4 회색+알파, 6 RGBA"""
    height, width = array.shape[:2]
    raw = b"".join(b"\x00" + array[y].astype(">u2").tobytes() for y in range(height))

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))

    header = struct.pack(">IIBBBBB", width, height, 16, color_type, 0, 0, 0)
    signature = b"\x89PNG\r\n\x1a\n"
    return signature + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def photo_array(width: int, height: int) -> np.ndarray:
    y, x = np.mgrid[0:height, 0:width]
    r = (x * 255 // max(width - 1, 1)).astype(np.uint8)
    g = (y * 255 // max(height - 1, 1)).astype(np.uint8)
    b = (((x // 37 + y // 29) % 2) * 200 + 30).astype(np.uint8)
    stripes = ((x + 2 * y) % 61 < 3)
    array = np.dstack([r, g, b])
    array[stripes] = (250, 250, 10)
    return array


def make_photos() -> dict[str, bytes]:
    photos: dict[str, bytes] = {}

    def save(name, image, fmt="PNG", **kw):
        buffer = io.BytesIO()
        image.save(buffer, fmt, **kw)
        photos[name] = buffer.getvalue()

    save("rgb_2100x1500.png", Image.fromarray(photo_array(2100, 1500)))
    save("rgb_5003x407.png", Image.fromarray(photo_array(5003, 407)))
    save("rgb_9001x301.png", Image.fromarray(photo_array(9001, 301)))
    save("rgb_1300x1000.png", Image.fromarray(photo_array(1300, 1000)))
    save("rgb_800x600.png", Image.fromarray(photo_array(800, 600)))
    save("rgb_600x2100.png", Image.fromarray(photo_array(600, 2100)))
    rgba = Image.fromarray(photo_array(1200, 1200)).convert("RGBA")
    alpha = Image.new("L", rgba.size, 0)
    ImageDraw.Draw(alpha).ellipse([100, 100, 1100, 1100], fill=255)
    ImageDraw.Draw(alpha).ellipse([450, 450, 750, 750], fill=100)
    rgba.putalpha(alpha)
    save("rgba_1200x1200.png", rgba)
    save("gray_1100x700.png", Image.fromarray(photo_array(1100, 700)).convert("L"))
    palette = Image.fromarray(photo_array(1200, 800)).convert("P", palette=Image.ADAPTIVE, colors=16)
    save("palette_1200x800.png", palette, transparency=0)
    exif = Image.Exif()
    exif[0x0112] = 6
    save("exif6_1500x1000.png", Image.fromarray(photo_array(1500, 1000)), exif=exif)
    exif8 = Image.Exif()
    exif8[0x0112] = 8
    save("exif8_rgba_1100x1050.png", rgba.resize((1100, 1050)), exif=exif8)
    exif3 = Image.Exif()
    exif3[0x0112] = 3
    save("exif3_900x500.png", Image.fromarray(photo_array(900, 500)), exif=exif3)
    exif5 = Image.Exif()
    exif5[0x0112] = 5
    save("exif5_1100x400.png", Image.fromarray(photo_array(1100, 400)), exif=exif5)
    save("webp_lossless_700x500.webp", Image.fromarray(photo_array(700, 500)), "WEBP", lossless=True)
    save("webp_lossy_700x500.webp", Image.fromarray(photo_array(700, 500)), "WEBP", quality=80)
    rng = np.random.RandomState(3)
    for name, color_type, channels in (("gray", 0, 1), ("rgb", 2, 3), ("graya", 4, 2), ("rgba", 6, 4)):
        values = rng.randint(0, 65536, (40, 50, channels)).astype(np.uint16)
        photos[f"{name}16_50x40.png"] = png16_bytes(values if channels > 1 else values[..., 0], color_type)
    jpeg_exif = Image.Exif()
    jpeg_exif[0x0112] = 6
    save("jpeg_exif6_400x300.jpg", Image.fromarray(photo_array(400, 300)), "JPEG", quality=92, exif=jpeg_exif)
    return photos


# ---------- 정답 ----------


def attempt(function):
    try:
        return function(), None
    except Exception as error:  # noqa: BLE001 : 어떤 예외든 정답에 오류 문구로 남긴다
        return None, str(error)


def gold_sheets(modules, sheets: Path, gold: Path) -> None:
    bake, canvas_mod, compose, engine = modules
    gold.mkdir(parents=True, exist_ok=True)
    bare = (sheets / "bare.png").read_bytes()
    worn = {p.stem.removeprefix("worn_"): p.read_bytes() for p in sorted(sheets.glob("worn_*.png"))}

    cut, error = attempt(lambda: engine.stand_front_cut(bare))
    if error:
        dump(gold / "cut.json", {"error": error})
        return
    cut.image.save(gold / "cut.png", optimize=True)
    dump(gold / "cut.json", {"left": cut.left, "top": cut.top})

    placement = canvas_mod.place(cut.image)
    if placement is None:
        dump(gold / "place.json", {"none": True})
        layout = canvas_mod.Layout(0.8, 40.0, 60.0)
    else:
        (gold / "place.png").write_bytes(placement.png)
        dump(
            gold / "place.json",
            {
                "eyes": [[fl(v) for v in point] for point in placement.eyes],
                "scale": fl(placement.scale),
                "origin": [fl(v) for v in placement.origin],
                "clipped": placement.clipped,
                "canvas_eyes": [[fl(v) for v in point] for point in placement.canvas_eyes],
            },
        )
        layout = placement.layout
    dump(gold / "layout.json", {"scale": fl(layout.scale), "origin_x": fl(layout.origin_x), "origin_y": fl(layout.origin_y)})

    base = bake.layer_base(bare)
    dump(gold / "layer_base.json", {"lit": base.lit, "left": base.cut.left, "top": base.cut.top})
    base.cut.image.save(gold / "layer_base.png", optimize=True)

    for regn, sheet_bytes in worn.items():
        source = bake.bake_source(bare, regn)
        lit = source != bare
        dump(gold / f"bake_source_{regn}.json", {"lit": lit})
        if lit:
            (gold / f"bake_source_{regn}.png").write_bytes(source)
        source = bake.bake_source(sheet_bytes, regn)
        dump(gold / f"bake_source_worn_{regn}.json", {"lit": source != sheet_bytes})
        if source != sheet_bytes:
            (gold / f"bake_source_worn_{regn}.png").write_bytes(source)

        _, error = attempt(lambda: bake.validate(bare, sheet_bytes, regn))
        dump(gold / f"validate_{regn}.json", {"error": error} if error else {"ok": True})

        worn_cut, error = attempt(lambda: engine.stand_front_cut(sheet_bytes))
        if error:
            dump(gold / f"worn_cut_{regn}.json", {"error": error})
            continue
        worn_cut.image.save(gold / f"worn_cut_{regn}.png", optimize=True)
        dump(gold / f"worn_cut_{regn}.json", {"left": worn_cut.left, "top": worn_cut.top})
        if regn == bake.REGN_HEAD:
            sprite = bake.worn_sprite(cut, worn_cut)
            dump(gold / f"sprite_{regn}.json", {"x": sprite.x, "y": sprite.y})
            continue
        sprite = bake.worn_sprite(base.cut, worn_cut)
        dump(gold / f"sprite_{regn}.json", {"x": sprite.x, "y": sprite.y})
        layer = bake.item_layer(base.cut.image, sprite, base.lit)
        if layer is None:
            dump(gold / f"layer_{regn}.json", {"none": True})
        else:
            layer.image.save(gold / f"layer_{regn}.png", optimize=True)
            dump(gold / f"layer_{regn}.json", {"x": layer.x, "y": layer.y})

    def render(name, selected):
        png, error = attempt(lambda: compose.render(bare, layout, {r: worn[r] for r in selected}))
        if error:
            dump(gold / f"render_{name}.json", {"error": error})
        else:
            (gold / f"render_{name}.png").write_bytes(png)
            dump(gold / f"render_{name}.json", {"ok": True})

    render("bare", [])
    if worn:
        render("all", list(worn))
    for regn in worn:
        render(regn, [regn])
    if "MYPC0001" in worn and len(worn) > 1:
        render("head_clothes", [r for r in ("MYPC0001", "MYPC0004") if r in worn])


def gold_items(engine, items: dict[str, bytes], out: Path) -> None:
    for name, raw in items.items():
        (out / f"{name}.png").write_bytes(raw)
        cut, error = attempt(lambda: engine.cut_item(raw))
        if error:
            dump(out / f"{name}.gold.json", {"error": error, "kind": type_of_error(error)})
        else:
            (out / f"{name}.gold.png").write_bytes(cut)


def type_of_error(message: str) -> str:
    return "item_image" if "그림이 없음" in message else "other"


def pixel_hash(png: bytes) -> dict:
    """사진 정규화 결과는 PNG 대신 크기와 화소 CRC32만 남긴다(저장소 크기) : RGB 8비트"""
    image = Image.open(io.BytesIO(png))
    assert image.mode == "RGB", image.mode
    return {"width": image.width, "height": image.height, "crc32": zlib.crc32(image.tobytes())}


def gold_photos(engine, photos: dict[str, bytes], out: Path, keep_png: bool) -> None:
    for name, raw in photos.items():
        (out / name).write_bytes(raw)
        for suffix, function in (("norm", engine.normalize_photo), ("normimg", engine.normalize_item_image)):
            result = function(raw)
            dump(out / f"{name}.{suffix}.json", pixel_hash(result))
            if keep_png:
                (out / f"{name}.{suffix}.png").write_bytes(result)


def rows(mask: np.ndarray) -> list[str]:
    return ["".join("1" if v else "0" for v in row) for row in mask]


def gold_ndimage(out: Path) -> None:
    """scipy.ndimage 결과 : 무작위 마스크마다 연산 결과를 문자열 행으로 남긴다"""
    rng = np.random.RandomState(7)
    cross = ndimage.generate_binary_structure(2, 1)
    square = np.ones((3, 3), dtype=bool)
    cases = []
    for shape, density in [((1, 1), 0.5), ((2, 3), 0.6), ((9, 7), 0.5), ((23, 17), 0.55), ((40, 31), 0.45), ((40, 31), 0.7), ((64, 64), 0.6), ((30, 50), 0.35)]:
        mask = rng.rand(*shape) < density
        allowed = rng.rand(*shape) < 0.75
        seeds = (rng.rand(*shape) < 0.08) & allowed
        labels, count = ndimage.label(mask)
        case = {
            "mask": rows(mask),
            "allowed": rows(allowed),
            "seeds": rows(seeds),
            "labels": labels.tolist(),
            "count": int(count),
            "fill_holes": rows(ndimage.binary_fill_holes(mask)),
            "erosion_cross_1": rows(ndimage.binary_erosion(mask)),
            "erosion_square_2": rows(ndimage.binary_erosion(mask, structure=square, iterations=2)),
            "dilation_cross_1": rows(ndimage.binary_dilation(mask)),
            "dilation_cross_3": rows(ndimage.binary_dilation(mask, iterations=3)),
            "dilation_square_2": rows(ndimage.binary_dilation(mask, structure=square, iterations=2)),
            "opening_cross_2": rows(ndimage.binary_opening(mask, iterations=2)),
            "opening_square_1": rows(ndimage.binary_opening(mask, structure=square)),
            "propagation": rows(ndimage.binary_propagation(seeds, mask=allowed)),
        }
        cases.append(case)
    dump(out / "ndimage.json", cases)


def load_modules(ai_repo: str):
    sys.path.insert(0, ai_repo)
    from app.character import bake, canvas, compose, engine

    return bake, canvas, compose, engine


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    args = [a for a in sys.argv if a != "--png"]
    if mode == "synth" and len(args) == 4:
        modules = load_modules(sys.argv[2])
        root = Path(sys.argv[3])
        for name, files in make_cases().items():
            folder = root / "sheets" / name
            folder.mkdir(parents=True, exist_ok=True)
            for filename, data in files.items():
                (folder / filename).write_bytes(data)
            gold_sheets(modules, folder, folder / "gold")
        (root / "items").mkdir(parents=True, exist_ok=True)
        gold_items(modules[3], make_items(), root / "items")
        (root / "photos").mkdir(parents=True, exist_ok=True)
        gold_photos(modules[3], make_photos(), root / "photos", "--png" in sys.argv)
        gold_ndimage(root)
        return 0
    if mode == "real" and len(sys.argv) == 5:
        modules = load_modules(sys.argv[2])
        gold_sheets(modules, Path(sys.argv[3]), Path(sys.argv[4]))
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
