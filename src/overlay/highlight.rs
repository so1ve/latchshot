use std::time::{Duration, Instant};

use crate::{AnimatedRect, Rect};

const SPRING_SETTLE_TIME: Duration = Duration::from_millis(120);
const FADE_TIME: Duration = Duration::from_millis(90);
const WINDOW_GAP_GRACE: Duration = Duration::from_millis(80);

/// A reveal generation awaiting each intersecting output's frame callback.
#[derive(Clone, Copy)]
pub(super) struct PendingReveal {
    pub(super) generation: u64,
    pub(super) target: Rect,
}

/// Animates the snap outline, fade-in, and brief gaps between windows.
pub(super) struct Highlight {
    animation: Option<SnapAnimation>,
    clear_at: Option<Instant>,
    reveal: Reveal,
    generation: u64,
    enabled: bool,
}

#[derive(Clone, Copy)]
enum Reveal {
    Waiting,
    Running(Instant),
    Finished,
}

impl Highlight {
    pub(super) const fn new(enabled: bool) -> Self {
        Self {
            animation: None,
            clear_at: None,
            reveal: Reveal::Waiting,
            generation: 0,
            enabled,
        }
    }

    pub(super) fn set_target(&mut self, target: Option<Rect>, now: Instant) {
        if self.clear_at.is_some_and(|deadline| now >= deadline) {
            self.clear();
        }

        let Some(target) = target else {
            if self.animation.is_some() {
                self.clear_at.get_or_insert(now + WINDOW_GAP_GRACE);
            }

            return;
        };

        match self.animation.as_mut() {
            Some(animation) if animation.target == target => {
                self.clear_at = None;
            }
            Some(animation) => {
                animation.sample(now);
                animation.target = target;
                self.clear_at = None;
                self.generation = self.generation.wrapping_add(1);
            }
            None => {
                self.animation = Some(SnapAnimation::new(target, now));
                self.clear_at = None;
                self.generation = self.generation.wrapping_add(1);
            }
        }
    }

    pub(super) fn sample(&mut self, now: Instant) -> (Option<Rect>, f32, bool) {
        if self.clear_at.is_some_and(|deadline| now >= deadline) {
            self.clear();

            return (None, 1.0, false);
        }
        let Some(animation) = &mut self.animation else {
            return (None, 1.0, false);
        };
        if !self.enabled {
            return (Some(animation.target), 1.0, self.clear_at.is_some());
        }
        let (rect, moving) = animation.sample(now);
        let reveal = match self.reveal {
            Reveal::Waiting => 0.0,
            Reveal::Running(started_at) => {
                let reveal = (now.duration_since(started_at).as_secs_f32()
                    / FADE_TIME.as_secs_f32())
                .min(1.0);
                if reveal == 1.0 {
                    self.reveal = Reveal::Finished;
                }

                reveal
            }
            Reveal::Finished => 1.0,
        };

        (
            Some(rect),
            reveal,
            moving || reveal < 1.0 || self.clear_at.is_some(),
        )
    }

    pub(super) const fn clear(&mut self) {
        self.animation = None;
        self.clear_at = None;
        self.reveal = Reveal::Waiting;
        self.generation = self.generation.wrapping_add(1);
    }

    pub(super) fn pending_reveal(&self) -> Option<PendingReveal> {
        if !self.enabled || !matches!(self.reveal, Reveal::Waiting) {
            return None;
        }
        let animation = self.animation.as_ref()?;

        Some(PendingReveal {
            generation: self.generation,
            target: animation.target,
        })
    }

    pub(super) const fn start_reveal(&mut self, now: Instant) {
        self.reveal = Reveal::Running(now);
    }
}

struct SnapAnimation {
    rect: AnimatedRect,
    target: Rect,
    last_frame: Instant,
}

impl SnapAnimation {
    fn new(target: Rect, now: Instant) -> Self {
        Self {
            rect: AnimatedRect::new(target, SPRING_SETTLE_TIME),
            target,
            last_frame: now,
        }
    }

    fn sample(&mut self, now: Instant) -> (Rect, bool) {
        let elapsed = now.duration_since(self.last_frame);
        self.last_frame = now;
        let rect = self.rect.advance(self.target, elapsed);

        if self.rect.is_settled(self.target) {
            (self.target, false)
        } else {
            (rect, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_animation_jumps_to_the_target() {
        let now = Instant::now();
        let target = Rect::new(10.0, 20.0, 300.0, 200.0);
        let mut highlight = Highlight::new(false);

        highlight.set_target(Some(target), now);

        assert_eq!(highlight.sample(now), (Some(target), 1.0, false));
    }

    #[test]
    fn highlight_reveals_after_reappearing() {
        let now = Instant::now();
        let first = Rect::new(10.0, 20.0, 300.0, 200.0);
        let second = Rect::new(400.0, 20.0, 300.0, 200.0);
        let mut highlight = Highlight::new(true);

        highlight.set_target(Some(first), now);
        assert_eq!(highlight.sample(now), (Some(first), 0.0, true));

        let much_later = now + Duration::from_secs(1);
        assert_eq!(highlight.sample(much_later), (Some(first), 0.0, true));

        highlight.start_reveal(much_later);
        assert_eq!(highlight.sample(much_later), (Some(first), 0.0, true));
        let after_fade = much_later + FADE_TIME;
        assert_eq!(highlight.sample(after_fade), (Some(first), 1.0, false));
        highlight.set_target(None, after_fade);
        let after_clear = after_fade + WINDOW_GAP_GRACE;
        assert_eq!(highlight.sample(after_clear), (None, 1.0, false));
        highlight.set_target(Some(second), after_clear);

        assert_eq!(highlight.sample(after_clear), (Some(second), 0.0, true));
        highlight.start_reveal(after_clear);
        assert_eq!(
            highlight.sample(after_clear + FADE_TIME),
            (Some(second), 1.0, false)
        );
    }

    #[test]
    fn interrupted_first_reveal_waits_for_the_next_highlight() {
        let now = Instant::now();
        let target = Rect::new(10.0, 20.0, 300.0, 200.0);
        let mut highlight = Highlight::new(true);

        highlight.set_target(Some(target), now);
        highlight.start_reveal(now);
        highlight.set_target(None, now);
        let after_clear = now + WINDOW_GAP_GRACE;
        assert_eq!(highlight.sample(after_clear), (None, 1.0, false));
        highlight.set_target(Some(target), after_clear);

        assert_eq!(highlight.sample(after_clear), (Some(target), 0.0, true));
    }

    #[test]
    fn window_gap_preserves_without_extending() {
        let now = Instant::now();
        let target = Rect::new(10.0, 20.0, 300.0, 200.0);
        let mut highlight = Highlight::new(false);

        highlight.set_target(Some(target), now);
        let generation = highlight.generation;
        highlight.set_target(None, now);
        highlight.set_target(None, now + WINDOW_GAP_GRACE / 2);

        assert_eq!(
            highlight.sample(now + WINDOW_GAP_GRACE / 2),
            (Some(target), 1.0, true)
        );
        assert_eq!(highlight.generation, generation);
        assert_eq!(highlight.sample(now + WINDOW_GAP_GRACE), (None, 1.0, false));
    }

    #[test]
    fn expired_window_gap_is_settled_before_a_new_target() {
        let now = Instant::now();
        let first = Rect::new(10.0, 20.0, 300.0, 200.0);
        let second = Rect::new(400.0, 20.0, 300.0, 200.0);
        let mut highlight = Highlight::new(true);

        highlight.set_target(Some(first), now);
        highlight.set_target(None, now);
        highlight.set_target(Some(second), now + WINDOW_GAP_GRACE);

        assert_eq!(
            highlight.sample(now + WINDOW_GAP_GRACE),
            (Some(second), 0.0, true)
        );
    }

    #[test]
    fn crossing_a_window_gap_keeps_the_transition_continuous() {
        let now = Instant::now();
        let first = Rect::new(10.0, 20.0, 300.0, 200.0);
        let second = Rect::new(400.0, 20.0, 300.0, 200.0);
        let mut highlight = Highlight::new(true);

        highlight.set_target(Some(first), now);
        highlight.start_reveal(now);
        let after_fade = now + FADE_TIME;
        highlight.sample(after_fade);
        highlight.set_target(None, after_fade);
        highlight.set_target(Some(second), after_fade + WINDOW_GAP_GRACE / 2);

        let (selection, reveal, animating) = highlight.sample(after_fade + WINDOW_GAP_GRACE);
        assert_ne!(selection, Some(second));
        assert_eq!(reveal, 1.0);
        assert!(animating);
        assert_eq!(
            highlight.sample(after_fade + Duration::from_secs(1)).0,
            Some(second)
        );
    }
}
