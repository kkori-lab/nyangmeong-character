# 이미지 계산 합성 정답

`tests/image_gold.rs`·`tests/ndimage_gold.rs`가 쓰는 입력과 정답이다. **실제 반려동물 사진·캐릭터 시트가 아니다**: `contract/gold_images.py`가 도형으로 그린 가짜 그림이다.

- `sheets/<사례>/` 2×2 시트(`bare.png`, `worn_<부위코드>.png`)와 `gold/`(원본 Python의 `engine`·`canvas`·`bake`·`compose` 결과 PNG·값)
- `items/` 아이템 그림과 `cut_item` 정답(`<이름>.gold.png`, 오류면 `<이름>.gold.json`)
- `photos/` 사진·아이템 이미지와 `normalize_photo`·`normalize_item_image` 정답(크기와 화소 CRC32, `*.norm.json`·`*.normimg.json`)
- `ndimage.json` 무작위 마스크의 scipy.ndimage 결과

원본 코드가 바뀌었을 때만 `sh scripts/gold.sh synth`로 다시 만들고, 만든 뒤 달라진 정답을 PR에서 확인한다. 정답 float는 repr 문자열이다(serde_json의 기본 실수 읽기는 마지막 한 비트가 틀릴 수 있다).
