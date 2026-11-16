//! `app/character/compose.py` : 착장 합성 (FN-MYP-011-01·FN-MYP-010-13)
//!
//! - 머리 착용 컷을 바탕으로 레이어를 얹는 방식 : `nyangmeong-ai` `docs/design/pixel-character-engine.md` 5번
//! - 외부 호출 없음

use std::collections::HashMap;
use std::time::Instant;

use crate::bake::{self, Sprite};
use crate::canvas::{CANVAS_SIZE, Layout};
use crate::engine::{self, EngineError};
use crate::logs;
use crate::pil::{self, Img};

/// MYPG01 공통코드 : 의상 → 가방 → 목 → 얼굴
pub const LAYER_ORDER: [&str; 4] = ["MYPC0004", "MYPC0005", "MYPC0003", "MYPC0002"];

fn paste_sprite(canvas: &mut Img, sprite: &Sprite, layout: &Layout) {
    let width = pil::py_round(sprite.image.width as f64 * layout.scale).max(1) as usize;
    let height = pil::py_round(sprite.image.height as f64 * layout.scale).max(1) as usize;
    // 도트 유지 : NEAREST
    let big = pil::resize_nearest(&sprite.image, width, height);
    let x = pil::py_round(layout.origin_x + sprite.x as f64 * layout.scale);
    let y = pil::py_round(layout.origin_y + sprite.y as f64 * layout.scale);
    pil::paste_masked(canvas, &big, x, y);
}

/// 부위 코드별 착용 시트를 입힌 512×512 PNG
pub fn render(
    bare_sheet: &[u8],
    layout: &Layout,
    worn_sheets: &HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, EngineError> {
    let started = Instant::now();
    let bare = engine::stand_front_cut(bare_sheet)?;
    let mut head = Sprite {
        image: bare.image.clone(),
        x: 0,
        y: 0,
    };
    if let Some(sheet) = worn_sheets.get(bake::REGN_HEAD) {
        head = bake::worn_sprite(&bare, &engine::stand_front_cut(sheet)?);
    }
    let mut canvas = Img::transparent(CANVAS_SIZE, CANVAS_SIZE);
    paste_sprite(&mut canvas, &head, layout);
    let base_done = started.elapsed();

    let layers: Vec<&str> = LAYER_ORDER
        .iter()
        .copied()
        .filter(|regn_cd| worn_sheets.contains_key(*regn_cd))
        .collect();
    if !layers.is_empty() {
        let base = bake::layer_base(bare_sheet)?;
        // 밝힌 시트의 컷은 맨몸 컷과 잘린 범위가 조금 다를 수 있음
        let shift_x = base.cut.left as i64 - bare.left as i64;
        let shift_y = base.cut.top as i64 - bare.top as i64;
        for regn_cd in layers {
            let worn = engine::stand_front_cut(&worn_sheets[regn_cd])?;
            let sprite = bake::worn_sprite(&base.cut, &worn);
            if let Some(layer) = bake::item_layer(&base.cut.image, &sprite, base.lit) {
                let shifted = Sprite {
                    x: layer.x + shift_x,
                    y: layer.y + shift_y,
                    ..layer
                };
                paste_sprite(&mut canvas, &shifted, layout);
            }
        }
    }
    let layers_done = started.elapsed();

    let png = pil::encode_png(&canvas);
    logs::info(
        "app.character.compose",
        format!(
            "합성 시간: 아이템 {}개, 바탕 {:.2}초, 레이어 {:.2}초, PNG {:.2}초",
            worn_sheets.len(),
            base_done.as_secs_f64(),
            (layers_done - base_done).as_secs_f64(),
            (started.elapsed() - layers_done).as_secs_f64(),
        ),
    );
    Ok(png)
}
