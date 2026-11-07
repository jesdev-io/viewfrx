//! In-memory console ring buffer behind the debug window. UI and service
//! workers push lines here; the debug window renders them. Never called from
//! the realtime audio callback.
use crate::config::runtime::LOG_CAPACITY;
use std::collections::VecDeque;
use std::sync::Mutex;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

pub fn push(line: String) {
    let mut lines = LINES.lock().unwrap();
    if lines.len() >= LOG_CAPACITY {
        lines.pop_front();
    }
    lines.push_back(line);
}

pub fn lines() -> Vec<String> {
    LINES.lock().unwrap().iter().cloned().collect()
}

macro_rules! logln {
    ($($arg:tt)*) => {
        $crate::logbuf::push(format!($($arg)*))
    };
}
pub(crate) use logln;
