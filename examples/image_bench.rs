//! 이미지 계산 측정 : 같은 시트로 `stand_front_cut`·`place`·`validate`·`render`의 실행 시간과 최대 힙을 잰다
//!
//! 사용: cargo run --release --example image_bench -- <시트 폴더> [반복 횟수=3]
//! - 시트 폴더: `bare.png`, `worn_<부위코드>.png` (`contract/image_bench.py`가 같은 폴더·같은 작업을 Python으로 잰다)
//! - 힙 : 이 프로그램의 전역 할당자가 센 값(작업 시작 시점 대비 최고치). 프로세스 전체(RSS)는 `/usr/bin/time -v`의 "Maximum resident set size"
//! - 서버에서는 Actions가 빌드한 실행 파일을 쓴다(서버에서 `cargo build`를 하지 않는다)

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use nyangmeong_character::{bake, canvas, compose, engine};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(bytes: usize) {
    let now = CURRENT.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(now, Relaxed);
}

// SAFETY: 모든 요청을 `System`에 그대로 넘기고 크기만 센다
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            grow(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            grow(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        CURRENT.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            if new_size >= layout.size() {
                grow(new_size - layout.size());
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Relaxed);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// (가장 빠른 시간 ms, 최대 힙 MB)
fn measure(repeat: usize, mut work: impl FnMut()) -> (f64, f64) {
    let mut best = f64::MAX;
    let mut peak = 0usize;
    for _ in 0..repeat {
        let base = CURRENT.load(Relaxed);
        PEAK.store(base, Relaxed);
        let started = Instant::now();
        work();
        best = best.min(started.elapsed().as_secs_f64() * 1000.0);
        peak = peak.max(PEAK.load(Relaxed).saturating_sub(base));
    }
    (best, peak as f64 / (1024.0 * 1024.0))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(
        args.get(1)
            .expect("사용: image_bench <시트 폴더> [반복 횟수]"),
    );
    let repeat: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(3);

    let bare = std::fs::read(dir.join("bare.png")).expect("bare.png");
    let mut worn: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in std::fs::read_dir(dir).expect("시트 폴더") {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if let Some(regn) = name
            .strip_prefix("worn_")
            .and_then(|n| n.strip_suffix(".png"))
        {
            worn.push((regn.to_string(), std::fs::read(&path).unwrap()));
        }
    }
    worn.sort();

    println!("작업\t시간(ms)\t최대 힙(MB)");
    let row = |name: &str, (ms, mb): (f64, f64)| println!("{name}\t{ms:.1}\t{mb:.1}");

    let cut = engine::stand_front_cut(&bare).expect("맨몸 컷");
    row(
        "stand_front_cut(맨몸)",
        measure(repeat, || {
            engine::stand_front_cut(&bare).unwrap();
        }),
    );
    let placement = canvas::place(&cut.image);
    row(
        "place",
        measure(repeat, || {
            canvas::place(&cut.image);
        }),
    );
    let layout = placement.map(|p| p.layout()).unwrap_or(canvas::Layout {
        scale: 1.0,
        origin_x: 0.0,
        origin_y: 0.0,
    });
    for (regn, sheet) in worn.iter().filter(|(regn, _)| regn != bake::REGN_HEAD) {
        row(
            &format!("validate({regn})"),
            measure(repeat, || {
                bake::validate(&bare, sheet, regn).unwrap();
            }),
        );
    }
    let all: HashMap<String, Vec<u8>> = worn.iter().cloned().collect();
    row(
        &format!("render(착용 {}개 전부)", all.len()),
        measure(repeat, || {
            compose::render(&bare, &layout, &all).unwrap();
        }),
    );
}
