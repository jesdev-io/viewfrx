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

/// Shared time-zoom window: all waveforms and spectrograms scale in tandem.
/// `None` in the app means "full file"; this struct always holds a concrete
/// clamped window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeWindow {
    pub start: f64,
    pub end: f64,
}
impl TimeWindow {
    pub fn full(duration: f64) -> Self {
        Self {
            start: 0.0,
            end: if duration.is_finite() { duration.max(0.0) } else { 0.0 },
        }
    }
    pub fn clamped(self, duration: f64) -> Self {
        let full = Self::full(duration);
        Self {
            start: self.start.clamp(0.0, full.end),
            end: self.end.clamp(self.start.clamp(0.0, full.end), full.end),
        }
    }
    pub fn len(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
    pub fn fraction_to_time(&self, fraction: f32) -> Option<f64> {
        (fraction.is_finite() && self.len() > 0.0)
            .then(|| self.start + fraction.clamp(0.0, 1.0) as f64 * self.len())
    }
    pub fn time_to_fraction(&self, seconds: f64) -> f32 {
        if self.len() <= 0.0 {
            return 0.0;
        }
        ((seconds - self.start) / self.len()).clamp(0.0, 1.0) as f32
    }
    /// Zoom in by `factor` around `center_fraction` of the current window.
    pub fn zoom_in(self, center_fraction: f32, factor: f64, min_len: f64, duration: f64) -> Self {
        let center = self.fraction_to_time(center_fraction).unwrap_or(self.start);
        let len = (self.len() / factor).max(min_len);
        let start = (center - len / 2.0).clamp(0.0, (duration - len).max(0.0));
        Self {
            start,
            end: (start + len).min(duration.max(0.0)),
        }
    }
    /// Zoom out by `factor`; returns None (full view) when the window covers
    /// the whole recording.
    pub fn zoom_out(
        self,
        center_fraction: f32,
        factor: f64,
        duration: f64,
    ) -> Option<TimeWindow> {
        let center = self.fraction_to_time(center_fraction).unwrap_or(self.start);
        let len = self.len() * factor;
        if len >= duration {
            return None;
        }
        let start = (center - len / 2.0).clamp(0.0, duration - len);
        Some(TimeWindow {
            start,
            end: start + len,
        })
    }
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

    #[test]
    fn time_window_zooms_in_tandem_and_resets_to_full() {
        let full = TimeWindow::full(10.0);
        assert_eq!(full.fraction_to_time(0.5), Some(5.0));
        let z = full.zoom_in(0.5, 2.0, 0.01, 10.0);
        assert_eq!(z, TimeWindow { start: 2.5, end: 7.5 });
        assert_eq!(z.fraction_to_time(0.0), Some(2.5));
        assert_eq!(z.time_to_fraction(5.0), 0.5);
        // Zooming out past the full duration resets to the full view.
        assert_eq!(z.zoom_out(0.5, 4.0, 10.0), None);
        assert_eq!(z.zoom_out(0.5, 2.0, 10.0), None);
        let z2 = z.zoom_out(0.5, 1.5, 10.0).unwrap();
        assert_eq!(z2, TimeWindow { start: 1.25, end: 8.75 });
        // Zoom-in clamps at the file edges and respects the minimum length.
        let edge = full.zoom_in(0.0, 2.0, 0.01, 10.0);
        assert_eq!(edge.start, 0.0);
        let tiny = TimeWindow { start: 0.0, end: 0.001 }.zoom_in(0.5, 2.0, 0.01, 10.0);
        assert!(tiny.len() >= 0.01);
        assert_eq!(TimeWindow::full(10.0).clamped(10.0).time_to_fraction(20.0), 1.0);
    }
}
