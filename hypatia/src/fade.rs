use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeDirection {
    Out,
    In,
}

/// Manages the fading of focus of the wallpaper.
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    fade_direction: Option<FadeDirection>,
    fade_start: Instant,
    last_time: Instant,
    duration: Duration,
    delta: Duration,
    current: f64,
}
impl Fade {
    /// Creates a new fader that is not fading
    pub fn new(fade_time: Duration) -> Self {
        Self {
            fade_direction: Some(FadeDirection::Out),
            fade_start: Instant::now(),
            last_time: Instant::now(),
            duration: fade_time,
            current: 1.0,
            delta: <_>::default(),
        }
    }
    pub fn direction(&self) -> Option<FadeDirection> {
        self.fade_direction
    }
    pub fn duration(&self) -> Duration {
        self.duration
    }
    pub fn duration_mut(&mut self) -> &mut Duration {
        &mut self.duration
    }
    fn stop_fade(&mut self) {
        self.fade_direction = None;
    }

    /// Continues the fade, returning a value between 0 and 1 representing the progress if currently fading
    pub fn continue_fade(&mut self) -> Option<f64> {
        let dest = match self.fade_direction? {
            FadeDirection::In => 1.0,
            FadeDirection::Out => 0.0,
        };
        let progress = match self.last_time.checked_duration_since(self.fade_start) {
            Some(x) => x,
            None => {
                // time travelling happened...
                self.current = dest;
                self.stop_fade();
                return Some(dest);
            }
        };
        if let Some(remaining_time) = self.duration.checked_sub(progress) {
            let remaining_progress = dest - self.current;
            let num_steps = remaining_time.as_secs_f64() / self.delta.as_secs_f64();
            let amount_per_step = remaining_progress / num_steps;
            self.current += amount_per_step;
            self.current = self.current.clamp(0.0, 1.0);
        } else {
            self.current = dest;
            self.stop_fade();
        }
        Some(self.current)
    }
    pub fn start_fade(&mut self, direction: FadeDirection) {
        if Some(direction) == self.fade_direction {
            return;
        }
        let now = Instant::now();
        self.fade_direction = Some(direction);
        self.last_time = now;
        self.delta = now - self.last_time;
        if (now.duration_since(self.fade_start)) < (self.duration / 2) {
            return;
        }
        self.fade_start = now;
    }
    pub fn update_delta(&mut self) {
        let now = Instant::now();
        self.delta = now - self.last_time;
        self.last_time = now;
    }
}
