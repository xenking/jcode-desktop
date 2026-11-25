//! The microphone level meter: a rolling window of RMS samples and the bar
//! height each one is drawn at. Shared by the panel's voice pill and the OS
//! pill, so both show the same curve.

/// Samples kept, oldest first. One bar is drawn for each.
pub const BARS: usize = 24;

/// The last [`BARS`] sampled levels and the loudest one since the capture
/// began. The peak is logged to tell silence from speech.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LevelMeter {
    levels: [f32; BARS],
    peak: f32,
}

impl LevelMeter {
    /// Append a sample, dropping the oldest.
    pub fn push(&mut self, level: f32) {
        self.levels.rotate_left(1);
        self.levels[BARS - 1] = level;
        self.peak = self.peak.max(level);
    }

    pub fn levels(&self) -> [f32; BARS] {
        self.levels
    }

    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// A deterministic waveform for offline previews, never a live reading.
    pub fn preview() -> Self {
        Self {
            levels: std::array::from_fn(|i| ((i as f32 * 0.7).sin().abs() * 0.2) + 0.002),
            peak: 0.,
        }
    }
}

/// Bar height for one level, between 2px (silence) and `max`. Speech RMS is
/// far below full scale, so the range is compressed: quiet speech shows
/// without making silence look like incoming audio. Invalid samples read as
/// silence.
pub fn bar_height(level: f32, max: f32) -> f32 {
    2.0 + (level.max(0.0) * 4.0).sqrt().min(1.0) * (max - 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_track_silence_and_clamp_loud_audio() {
        assert_eq!(bar_height(0., 18.), 2.);
        assert_eq!(bar_height(-1., 18.), 2.);
        assert_eq!(bar_height(f32::NAN, 18.), 2.);
        assert_eq!(bar_height(f32::NEG_INFINITY, 18.), 2.);
        assert_eq!(bar_height(f32::INFINITY, 18.), 18.);
        assert_eq!(bar_height(1., 18.), 18.);
        assert_eq!(bar_height(2., 18.), 18.);
        assert!(bar_height(0.1, 18.) > bar_height(0.01, 18.));
        assert!(bar_height(0.02, 18.) > 6.);
    }

    #[test]
    fn push_keeps_the_newest_samples_and_the_loudest_peak() {
        let mut meter = LevelMeter::default();
        meter.push(0.5);
        meter.push(0.1);
        let levels = meter.levels();
        assert_eq!(levels[BARS - 2], 0.5);
        assert_eq!(levels[BARS - 1], 0.1);
        assert_eq!(meter.peak(), 0.5);
        for _ in 0..BARS {
            meter.push(0.);
        }
        assert_eq!(meter.levels(), [0.; BARS]);
        assert_eq!(meter.peak(), 0.5);
    }
}
