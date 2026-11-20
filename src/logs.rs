//! `app/core/logs.py` 콘솔 형식 `%(levelname)s:     %(name)s - %(message)s`, `configure`로 LOG_DIR을 주면
//! `app.*` 로그를 `LOG_DIR/character_YYYY-MM-DD.log`에도 씀(`DailyFileHandler`, 30일 보관). systemd는 stderr를 journald에 남김

use std::fmt::Display;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use chrono::{Local, NaiveDate, TimeDelta, Weekday};

/// 원본은 `ai_` : 1번 Python 파일과 구별(migration.md "알려진 차이")
pub const FILE_PREFIX: &str = "character_";
pub const KEEP_DAYS: i64 = 30;
pub const FILE_MODE: u32 = 0o660;
pub const DIR_MODE: u32 = 0o2770;

const LOGGER: &str = "app.logs";

/// 오늘 날짜(서버 지역 시각) : 테스트는 바꿔 끼움
pub type Today = Box<dyn Fn() -> NaiveDate + Send>;

/// 프로세스 하나라 파일 잠금 대신 Mutex로 한 줄씩 씀
static FILE: Mutex<Option<DailyFile>> = Mutex::new(None);

pub fn local_today() -> NaiveDate {
    Local::now().date_naive()
}

fn write(level: &str, name: &str, message: impl Display) {
    eprintln!("{level}:     {name} - {message}");
    // 파일에 남길 로거 : app.* (지금 로그는 전부 INFO 이상)
    if name != "app" && !name.starts_with("app.") {
        return;
    }
    if let Some(file) = file_guard().as_mut() {
        file.emit(level, name, &message.to_string());
    }
}

fn file_guard() -> MutexGuard<'static, Option<DailyFile>> {
    FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn info(name: &str, message: impl Display) {
    write("INFO", name, message);
}

pub fn warning(name: &str, message: impl Display) {
    write("WARNING", name, message);
}

pub fn error(name: &str, message: impl Display) {
    write("ERROR", name, message);
}

/// `DailyFileHandler` : LOG_DIR/character_YYYY-MM-DD.log 이어 쓰기, 날짜가 바뀌면 새 파일
pub struct DailyFile {
    folder: PathBuf,
    today: Today,
    day: NaiveDate,
    file: File,
}

impl DailyFile {
    pub fn open(folder: &Path, today: Today) -> io::Result<Self> {
        let day = today();
        let file = open_day(folder, day)?;
        remove_old_files(folder, day);
        Ok(Self {
            folder: folder.to_path_buf(),
            today,
            day,
            file,
        })
    }

    fn switch(&mut self, day: NaiveDate) -> io::Result<()> {
        // 새 파일을 연 뒤 이전 파일을 닫음(대입하면 이전 File이 닫힘)
        self.file = open_day(&self.folder, day)?;
        self.day = day;
        remove_old_files(&self.folder, day);
        Ok(())
    }

    /// `시각.밀리초 [pid] 레벨 로거 - 메시지` (`FILE_FORMAT`, 시각은 지역 `%H:%M:%S`)
    pub fn emit(&mut self, level: &str, name: &str, message: &str) {
        let now = Local::now();
        let line = format!(
            "{} [{}] {level} {name} - {message}\n",
            now.format("%H:%M:%S%.3f"),
            std::process::id()
        );
        let result = (|| {
            let day = (self.today)();
            if day != self.day {
                self.switch(day)?;
            }
            self.file.write_all(line.as_bytes())?;
            self.file.flush()
        })();
        // logging.Handler.handleError : stderr에 알리고 계속
        if let Err(err) = result {
            eprintln!("--- Logging error --- {err}");
        }
    }
}

fn open_day(folder: &Path, day: NaiveDate) -> io::Result<File> {
    let path = folder.join(format!("{FILE_PREFIX}{}.log", day.format("%Y-%m-%d")));
    let is_new = !path.exists();
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    if is_new {
        chmod(&path, FILE_MODE);
    }
    Ok(file)
}

/// KEEP_DAYS 지난 character_*.log만 지움 : 이름이 날짜가 아니면 둠
pub fn remove_old_files(folder: &Path, today: NaiveDate) {
    let limit = today - TimeDelta::days(KEEP_DAYS);
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(stem) = name
            .strip_prefix(FILE_PREFIX)
            .and_then(|rest| rest.strip_suffix(".log"))
        else {
            continue;
        };
        let Some(day) = from_isoformat(stem) else {
            continue;
        };
        if day < limit {
            // 지우기 실패는 무시(원본 contextlib.suppress(OSError))
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Python 3.13 `date.fromisoformat` : YYYY-MM-DD·YYYYMMDD·YYYY-Www-D·YYYYWwwD·YYYY-Www·YYYYWww(ASCII 숫자, 1~9999년)
pub fn from_isoformat(text: &str) -> Option<NaiveDate> {
    let b = text.as_bytes();
    if !matches!(b.len(), 7 | 8 | 10) {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = b.get(range)?;
        if part.is_empty() || !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(part.iter().fold(0, |n, d| n * 10 + u32::from(d - b'0')))
    };
    let year = num(0..4)?;
    let sep = b.get(4) == Some(&b'-');
    let mut pos = 4 + usize::from(sep);
    let date = if b.get(pos) == Some(&b'W') {
        pos += 1;
        let week = num(pos..pos + 2)?;
        pos += 2;
        let weekday = if pos < b.len() {
            if sep {
                if b[pos] != b'-' {
                    return None;
                }
                pos += 1;
            }
            let day = num(pos..pos + 1)?;
            pos += 1;
            day
        } else {
            1
        };
        if !(1..=7).contains(&weekday) {
            return None;
        }
        let weekday = Weekday::try_from(weekday as u8 - 1).ok()?;
        NaiveDate::from_isoywd_opt(year as i32, week, weekday)?
    } else {
        let month = num(pos..pos + 2)?;
        pos += 2;
        if sep {
            if b.get(pos) != Some(&b'-') {
                return None;
            }
            pos += 1;
        }
        let day = num(pos..pos + 2)?;
        pos += 2;
        NaiveDate::from_ymd_opt(year as i32, month, day)?
    };
    if pos != b.len() || year == 0 || !(1..=9999).contains(&chrono::Datelike::year(&date)) {
        return None;
    }
    Some(date)
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    // 소유자가 아니면 폴더 setgid·umask를 따름
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn chmod(_path: &Path, _mode: u32) {}

/// `_open_folder` : 없으면 만들고(부모까지) 그때만 2770
fn open_folder(log_dir: &str, today: Today) -> io::Result<DailyFile> {
    let folder = Path::new(log_dir);
    let is_new = !folder.exists();
    fs::create_dir_all(folder)?;
    if is_new {
        chmod(folder, DIR_MODE);
    }
    DailyFile::open(folder, today)
}

/// `configure` : 다시 불러도 파일이 한 벌, LOG_DIR이 비면 콘솔만
pub fn configure(log_dir: &str, today: Today) {
    if let Some(message) = try_configure(log_dir, today) {
        warning(LOGGER, message);
    }
}

/// 폴더를 못 열면 남길 경고 문구를 돌려줌(콘솔에만 남음 : 그 전에 파일을 닫음)
fn try_configure(log_dir: &str, today: Today) -> Option<String> {
    *file_guard() = None;
    if log_dir.is_empty() {
        return None;
    }
    match open_folder(log_dir, today) {
        Ok(file) => {
            *file_guard() = Some(file);
            None
        }
        Err(err) => Some(format!("로그 폴더에 쓸 수 없어 콘솔에만 기록: {err}")),
    }
}

#[cfg(test)]
mod tests {
    // 원본 tests/core/test_logs.py
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    /// 전역 파일을 건드리는 테스트끼리 차례로
    static SERIAL: Mutex<()> = Mutex::new(());

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 9).unwrap()
    }

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("nm-logs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    struct Reset<'a>(#[allow(dead_code)] MutexGuard<'a, ()>);

    impl Drop for Reset<'_> {
        fn drop(&mut self) {
            configure("", Box::new(local_today));
        }
    }

    fn serial() -> Reset<'static> {
        Reset(SERIAL.lock().unwrap_or_else(|p| p.into_inner()))
    }

    fn is_line(line: &str, level: &str, name: &str, message: &str) -> bool {
        // ^\d{2}:\d{2}:\d{2}\.\d{3} \[\d+\] INFO app\.test - hello$
        let b = line.as_bytes();
        let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
        if b.len() < 14 || !digits(0..2) || b[2] != b':' || !digits(3..5) || b[5] != b':' {
            return false;
        }
        if !digits(6..8) || b[8] != b'.' || !digits(9..12) || &line[12..14] != " [" {
            return false;
        }
        let Some((pid, rest)) = line[14..].split_once("] ") else {
            return false;
        };
        !pid.is_empty()
            && pid.bytes().all(|c| c.is_ascii_digit())
            && rest == format!("{level} {name} - {message}")
    }

    fn lines_with(path: &Path, message: &str) -> Vec<String> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter(|line| line.ends_with(&format!(" - {message}")))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn empty_log_dir_has_no_file() {
        let _reset = serial();
        configure("", Box::new(day));
        assert!(file_guard().is_none());
    }

    #[test]
    fn writes_dated_file_with_java_like_line() {
        let _reset = serial();
        let folder = temp("dated").join("logs");
        configure(folder.to_str().unwrap(), Box::new(day));

        info("app.test", "hello-dated");

        let lines = lines_with(&folder.join("character_2026-11-09.log"), "hello-dated");
        assert_eq!(lines.len(), 1);
        assert!(
            is_line(&lines[0], "INFO", "app.test", "hello-dated"),
            "{}",
            lines[0]
        );
        warning("app", "boot-warn");
        info("other.test", "not-app");
        let path = folder.join("character_2026-11-09.log");
        assert!(is_line(
            &lines_with(&path, "boot-warn")[0],
            "WARNING",
            "app",
            "boot-warn"
        ));
        assert!(lines_with(&path, "not-app").is_empty());
    }

    #[test]
    fn date_change_opens_new_file_and_removes_old() {
        let _reset = serial();
        let folder = temp("change");
        fs::create_dir_all(&folder).unwrap();
        let next_day = day() + TimeDelta::days(1);
        let name = |d: NaiveDate| folder.join(format!("character_{}.log", d.format("%Y-%m-%d")));
        let old = name(next_day - TimeDelta::days(31));
        let kept = name(next_day - TimeDelta::days(30));
        let other = folder.join("other.log");
        let not_date = folder.join("character_backup.log");
        for path in [&old, &kept, &other, &not_date] {
            fs::write(path, "x").unwrap();
        }
        let offset = Arc::new(AtomicI64::new(0));
        let shared = offset.clone();
        configure(
            folder.to_str().unwrap(),
            Box::new(move || day() + TimeDelta::days(shared.load(Ordering::SeqCst))),
        );
        assert!(old.exists()); // 첫날 기준으로는 30일 이내

        offset.store(1, Ordering::SeqCst);
        info("app.test", "hello-next");

        let lines = lines_with(&folder.join("character_2026-11-10.log"), "hello-next");
        assert!(is_line(&lines[0], "INFO", "app.test", "hello-next"));
        assert!(!old.exists());
        assert!(kept.exists() && other.exists() && not_date.exists());
    }

    #[test]
    fn unwritable_dir_keeps_console_only() {
        let _reset = serial();
        let folder = temp("blocked");
        fs::create_dir_all(&folder).unwrap();
        let blocked = folder.join("file");
        fs::write(&blocked, "x").unwrap();

        let warned = try_configure(blocked.to_str().unwrap(), Box::new(day));

        assert!(file_guard().is_none());
        let warned = warned.expect("경고가 있어야 함");
        assert!(
            warned.starts_with("로그 폴더에 쓸 수 없어 콘솔에만 기록: "),
            "{warned}"
        );
    }

    #[test]
    fn configure_twice_writes_one_line() {
        let _reset = serial();
        let folder = temp("twice");
        configure(folder.to_str().unwrap(), Box::new(day));
        configure(folder.to_str().unwrap(), Box::new(day));

        info("app.test", "hello-twice");

        let lines = lines_with(&folder.join("character_2026-11-09.log"), "hello-twice");
        assert_eq!(lines.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn new_file_mode_is_660_and_new_dir_is_2770() {
        use std::os::unix::fs::PermissionsExt;
        let _reset = serial();
        let folder = temp("mode").join("logs");
        configure(folder.to_str().unwrap(), Box::new(day));

        let file = fs::metadata(folder.join("character_2026-11-09.log")).unwrap();
        assert_eq!(file.permissions().mode() & 0o777, 0o660);
        assert_eq!(
            fs::metadata(&folder).unwrap().permissions().mode() & 0o7777,
            0o2770
        );
    }

    #[test]
    fn from_isoformat_matches_python() {
        // Python 3.13 date.fromisoformat 실측(2026-11-17)
        let ok = [
            ("2026-11-09", (2026, 11, 9)),
            ("20261109", (2026, 11, 9)),
            ("2026-W45-1", (2026, 11, 2)),
            ("2026W451", (2026, 11, 2)),
            ("2026-W45", (2026, 11, 2)),
            ("2026W45", (2026, 11, 2)),
            ("0001-01-01", (1, 1, 1)),
            ("2026-W53-1", (2026, 12, 28)),
            ("2020-W53-1", (2020, 12, 28)),
            ("0001-W01-1", (1, 1, 1)),
        ];
        for (text, (y, m, d)) in ok {
            assert_eq!(
                from_isoformat(text),
                NaiveDate::from_ymd_opt(y, m, d),
                "{text}"
            );
        }
        let bad = [
            "2026-11",
            "2026-1109",
            "202611-09",
            "2026-11-9",
            "2026-02-30",
            "0000-01-01",
            "2026-W00-1",
            "2026-W45-8",
            "2026-W45-0",
            "2026-W451",
            "2026W45-1",
            "2026-11-09 ",
            " 2026-11-09",
            "2026-11-09T00",
            "2026-1-1",
            "backup",
            "",
            "9999-W52-7",
            "2026-W45-1x",
            "٢٠٢٦-١١-٠٩",
        ];
        for text in bad {
            assert_eq!(from_isoformat(text), None, "{text}");
        }
    }
}
