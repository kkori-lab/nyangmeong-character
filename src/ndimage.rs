//! scipy.ndimage 1.18.1 : `label`·`binary_fill_holes`·`binary_erosion`·`binary_dilation`·`binary_opening`·`binary_propagation`
//!
//! 이미지 계산(`engine.py`·`canvas.py`·`bake.py`)이 쓰는 모양만 옮김. 마스크는 `h × w` 행 우선 `bool`
//! - 구조는 기본 십자(4연결) 또는 3×3 전체. 이미지 밖은 항상 0(`border_value=0`)

/// scipy 구조 요소
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Structure {
    /// `generate_binary_structure(2, 1)` : scipy의 기본값
    Cross,
    /// `np.ones((3, 3))`
    Square,
}

const CROSS: [(isize, isize); 5] = [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)];

fn offsets(structure: Structure) -> Vec<(isize, isize)> {
    match structure {
        Structure::Cross => CROSS.to_vec(),
        Structure::Square => (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dy, dx)))
            .collect(),
    }
}

/// `ndimage.label` (4연결) : 번호는 래스터 순서로 처음 만나는 덩어리부터 1, 2, …
pub fn label(mask: &[bool], width: usize, height: usize) -> (Vec<u32>, usize) {
    let mut labels = vec![0u32; mask.len()];
    let mut count = 0u32;
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || labels[start] != 0 {
            continue;
        }
        count += 1;
        labels[start] = count;
        stack.push(start);
        while let Some(at) = stack.pop() {
            let (y, x) = (at / width, at % width);
            let mut visit = |next: usize| {
                if mask[next] && labels[next] == 0 {
                    labels[next] = count;
                    stack.push(next);
                }
            };
            if y > 0 {
                visit(at - width);
            }
            if y + 1 < height {
                visit(at + width);
            }
            if x > 0 {
                visit(at - 1);
            }
            if x + 1 < width {
                visit(at + 1);
            }
        }
    }
    (labels, count as usize)
}

/// `ndimage.sum(values, labels, range(1, count + 1))` : `values`가 참인 화소 수(덩어리마다)
pub fn count_by_label(values: Option<&[bool]>, labels: &[u32], count: usize) -> Vec<u64> {
    let mut sums = vec![0u64; count];
    for (at, &id) in labels.iter().enumerate() {
        if id != 0 && values.is_none_or(|values| values[at]) {
            sums[id as usize - 1] += 1;
        }
    }
    sums
}

/// `ndimage.binary_dilation(mask, structure, iterations)`
pub fn binary_dilation(
    mask: &[bool],
    width: usize,
    height: usize,
    structure: Structure,
    iterations: usize,
) -> Vec<bool> {
    let offsets = offsets(structure);
    let mut current = mask.to_vec();
    for _ in 0..iterations {
        let mut next = vec![false; current.len()];
        for y in 0..height {
            for x in 0..width {
                next[y * width + x] = offsets.iter().any(|&(dy, dx)| {
                    let (ny, nx) = (y as isize + dy, x as isize + dx);
                    ny >= 0
                        && nx >= 0
                        && (ny as usize) < height
                        && (nx as usize) < width
                        && current[ny as usize * width + nx as usize]
                });
            }
        }
        current = next;
    }
    current
}

/// `ndimage.binary_erosion(mask, structure, iterations)` : 이미지 밖은 0이라 가장자리 화소는 깎임
pub fn binary_erosion(
    mask: &[bool],
    width: usize,
    height: usize,
    structure: Structure,
    iterations: usize,
) -> Vec<bool> {
    let offsets = offsets(structure);
    let mut current = mask.to_vec();
    for _ in 0..iterations {
        let mut next = vec![false; current.len()];
        for y in 0..height {
            for x in 0..width {
                next[y * width + x] = offsets.iter().all(|&(dy, dx)| {
                    let (ny, nx) = (y as isize + dy, x as isize + dx);
                    ny >= 0
                        && nx >= 0
                        && (ny as usize) < height
                        && (nx as usize) < width
                        && current[ny as usize * width + nx as usize]
                });
            }
        }
        current = next;
    }
    current
}

/// `ndimage.binary_opening(mask, structure, iterations)` : 깎기 `iterations`번 뒤 늘리기 `iterations`번
pub fn binary_opening(
    mask: &[bool],
    width: usize,
    height: usize,
    structure: Structure,
    iterations: usize,
) -> Vec<bool> {
    let eroded = binary_erosion(mask, width, height, structure, iterations);
    binary_dilation(&eroded, width, height, structure, iterations)
}

/// 씨앗에서 `allowed` 안으로 4연결로 번지게 한 결과 (`ndimage.binary_propagation(seeds, mask=allowed)`)
pub fn binary_propagation(
    seeds: &[bool],
    allowed: &[bool],
    width: usize,
    height: usize,
) -> Vec<bool> {
    let mut reached = vec![false; seeds.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (at, &seed) in seeds.iter().enumerate() {
        if seed && allowed[at] {
            reached[at] = true;
            stack.push(at);
        }
    }
    while let Some(at) = stack.pop() {
        let (y, x) = (at / width, at % width);
        let mut visit = |next: usize| {
            if allowed[next] && !reached[next] {
                reached[next] = true;
                stack.push(next);
            }
        };
        if y > 0 {
            visit(at - width);
        }
        if y + 1 < height {
            visit(at + width);
        }
        if x > 0 {
            visit(at - 1);
        }
        if x + 1 < width {
            visit(at + 1);
        }
    }
    reached
}

/// `ndimage.binary_fill_holes(mask)` : 가장자리와 4연결로 이어진 배경 밖은 모두 채움
pub fn binary_fill_holes(mask: &[bool], width: usize, height: usize) -> Vec<bool> {
    let background: Vec<bool> = mask.iter().map(|&on| !on).collect();
    let mut seeds = vec![false; mask.len()];
    for y in 0..height {
        for x in 0..width {
            if y == 0 || x == 0 || y + 1 == height || x + 1 == width {
                seeds[y * width + x] = true;
            }
        }
    }
    let outside = binary_propagation(&seeds, &background, width, height);
    outside.iter().map(|&out| !out).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&str]) -> (Vec<bool>, usize, usize) {
        let height = rows.len();
        let width = rows[0].len();
        let cells = rows
            .iter()
            .flat_map(|row| row.bytes().map(|c| c == b'#'))
            .collect();
        (cells, width, height)
    }

    fn show(mask: &[bool], width: usize) -> Vec<String> {
        mask.chunks(width)
            .map(|row| row.iter().map(|&on| if on { '#' } else { '.' }).collect())
            .collect()
    }

    #[test]
    fn label_numbers_in_raster_order_with_four_connectivity() {
        let (mask, w, h) = grid(&["#.#", ".#.", "#.#"]);
        let (labels, count) = label(&mask, w, h);
        // 대각선은 이어지지 않음: 5개
        assert_eq!(count, 5);
        assert_eq!(labels, vec![1, 0, 2, 0, 3, 0, 4, 0, 5]);
    }

    #[test]
    fn fill_holes_fills_only_enclosed_background() {
        let (mask, w, h) = grid(&["#####", "#...#", "#.#.#", "#...#", "#####"]);
        let filled = binary_fill_holes(&mask, w, h);
        assert!(filled.iter().all(|&on| on));
        // 가장자리에 열린 곳이 있으면 채우지 않음
        let (open, w, h) = grid(&["#####", "#...#", "#....", "#...#", "#####"]);
        assert_eq!(binary_fill_holes(&open, w, h), open);
    }

    #[test]
    fn erosion_removes_image_border_pixels() {
        let (mask, w, h) = grid(&["###", "###", "###"]);
        let eroded = binary_erosion(&mask, w, h, Structure::Cross, 1);
        assert_eq!(show(&eroded, w), vec!["...", ".#.", "..."]);
    }

    #[test]
    fn dilation_grows_by_structure_and_iterations() {
        let (mask, w, h) = grid(&[".....", ".....", "..#..", ".....", "....."]);
        assert_eq!(
            show(&binary_dilation(&mask, w, h, Structure::Cross, 1), w),
            vec![".....", "..#..", ".###.", "..#..", "....."]
        );
        assert_eq!(
            show(&binary_dilation(&mask, w, h, Structure::Square, 1), w),
            vec![".....", ".###.", ".###.", ".###.", "....."]
        );
        assert_eq!(
            show(&binary_dilation(&mask, w, h, Structure::Cross, 2), w),
            vec!["..#..", ".###.", "#####", ".###.", "..#.."]
        );
    }

    #[test]
    fn opening_drops_specks_and_keeps_blocks() {
        let (mask, w, h) = grid(&["#......", "...###.", "...###.", "...###.", "......."]);
        let opened = binary_opening(&mask, w, h, Structure::Square, 1);
        assert_eq!(
            show(&opened, w),
            vec![".......", "...###.", "...###.", "...###.", "......."]
        );
    }

    #[test]
    fn propagation_spreads_only_through_allowed() {
        let (allowed, w, h) = grid(&["###..#", "..#..#", "..####"]);
        let seeds: Vec<bool> = (0..allowed.len()).map(|at| at == 0).collect();
        let reached = binary_propagation(&seeds, &allowed, w, h);
        assert_eq!(show(&reached, w), vec!["###..#", "..#..#", "..####"]);
        // 이어지지 않은 곳은 번지지 않음
        let (allowed, w, h) = grid(&["##.#"]);
        let seeds = vec![true, false, false, false];
        assert_eq!(
            show(&binary_propagation(&seeds, &allowed, w, h), w),
            vec!["##.."]
        );
    }

    #[test]
    fn counts_pixels_per_label() {
        let (mask, w, h) = grid(&["##.#", "...#"]);
        let (labels, count) = label(&mask, w, h);
        assert_eq!(count_by_label(None, &labels, count), vec![2, 2]);
        let only = vec![true, false, false, true, false, false, false, false];
        assert_eq!(count_by_label(Some(&only), &labels, count), vec![1, 1]);
    }
}
