use crate::services::Entry;
use std::path::PathBuf;

#[derive(Default)]
pub struct Browser {
    pub entries: Vec<Entry>,
    pub visible: Vec<usize>,
    pub query: String,
    pub selected: Option<PathBuf>,
}
impl Browser {
    pub fn append(&mut self, entries: Vec<Entry>) {
        let query = self.query.to_lowercase();
        for entry in entries {
            if entry.search.contains(&query) {
                self.visible.push(self.entries.len());
            }
            self.entries.push(entry);
        }
    }
    pub fn filter(&mut self) {
        let query = self.query.to_lowercase();
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.search.contains(&query).then_some(i))
            .collect();
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.visible.clear();
    }
    pub fn adjacent(&self, direction: i32) -> Option<PathBuf> {
        if self.visible.is_empty() {
            return None;
        }
        let current = self
            .visible
            .iter()
            .position(|&i| Some(&self.entries[i].path) == self.selected.as_ref());
        let next = match current {
            Some(i) => (i as i32 + direction).clamp(0, self.visible.len() as i32 - 1) as usize,
            None => 0,
        };
        Some(self.entries[self.visible[next]].path.clone())
    }
}

pub fn seek_at(fraction: f32, duration: f64) -> Option<f64> {
    (fraction.is_finite() && duration.is_finite() && duration > 0.0)
        .then(|| fraction.clamp(0.0, 1.0) as f64 * duration)
}

pub fn clock(seconds: f64) -> String {
    let seconds = if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    };
    let minutes = seconds as u64 / 60;
    format!("{minutes:02}:{:06.3}", seconds % 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_applies_to_new_batches_and_navigation_stays_visible() {
        let mut model = Browser {
            query: "RAIN".into(),
            ..Default::default()
        };
        model.append(vec![
            Entry::new("rain.wav".into()),
            Entry::new("wind.wav".into()),
        ]);
        model.append(vec![Entry::new("Rain on roof.mp3".into())]);
        assert_eq!(model.visible, [0, 2]);
        model.selected = Some("rain.wav".into());
        assert_eq!(model.adjacent(1), Some("Rain on roof.mp3".into()));
        model.query = "no match".into();
        model.filter();
        assert_eq!(model.adjacent(1), None);
    }
    #[test]
    fn seeking_and_clock_handle_invalid_or_empty_audio() {
        assert_eq!(seek_at(0.5, 10.0), Some(5.0));
        assert_eq!(seek_at(2.0, 10.0), Some(10.0));
        assert_eq!(seek_at(0.5, 0.0), None);
        assert_eq!(seek_at(f32::NAN, 1.0), None);
        assert_eq!(clock(65.25), "01:05.250");
    }
}
