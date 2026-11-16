//! `app/core/logs.py` 콘솔 형식 `%(levelname)s:     %(name)s - %(message)s`
//! 날짜별 파일은 두지 않는다 : systemd가 stderr를 journald에 남긴다

use std::fmt::Display;

fn write(level: &str, name: &str, message: impl Display) {
    eprintln!("{level}:     {name} - {message}");
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
