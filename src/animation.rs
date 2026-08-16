use std::time::Duration;

use crate::Rect;

const SETTLE_EXPONENT: f64 = 10.0;
const POSITION_EPSILON: f64 = 0.1;
const VELOCITY_EPSILON: f64 = 5.0;

#[derive(Debug, Clone, Copy)]
struct Spring {
    value: f64,
    velocity: f64,
}

impl Spring {
    const fn new(value: f64) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }

    fn advance(&mut self, target: f64, omega: f64, seconds: f64) {
        let error = self.value - target;
        let coefficient = self.velocity + omega * error;
        let decay = (-omega * seconds).exp();

        self.value = target + (error + coefficient * seconds) * decay;
        self.velocity = (self.velocity - omega * coefficient * seconds) * decay;
    }

    fn is_settled(self, target: f64) -> bool {
        (self.value - target).abs() <= POSITION_EPSILON && self.velocity.abs() <= VELOCITY_EPSILON
    }
}

/// A frame-rate-independent, critically damped rectangle animation.
pub struct AnimatedRect {
    components: [Spring; 4],
    omega: f64,
}

impl AnimatedRect {
    #[must_use]
    pub fn new(rect: Rect, settle_time: Duration) -> Self {
        debug_assert!(!settle_time.is_zero());

        Self {
            components: [rect.left(), rect.top(), rect.width(), rect.height()].map(Spring::new),
            omega: SETTLE_EXPONENT / settle_time.as_secs_f64(),
        }
    }

    pub fn advance(&mut self, target: Rect, elapsed: Duration) -> Rect {
        let targets = [target.left(), target.top(), target.width(), target.height()];
        for (spring, target) in self.components.iter_mut().zip(targets) {
            spring.advance(target, self.omega, elapsed.as_secs_f64());
        }
        let [x, y, width, height] = self.components.map(|spring| spring.value);

        Rect::new(x, y, width, height)
    }

    #[must_use]
    pub fn is_settled(&self, target: Rect) -> bool {
        let targets = [target.left(), target.top(), target.width(), target.height()];

        self.components
            .iter()
            .zip(targets)
            .all(|(spring, target)| spring.is_settled(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_without_frame_rate_dependence() {
        let start = Rect::new(0.0, 0.0, 100.0, 100.0);
        let target = Rect::new(300.0, 200.0, 800.0, 600.0);
        let mut one_step = AnimatedRect::new(start, Duration::from_millis(120));
        let mut two_steps = AnimatedRect::new(start, Duration::from_millis(120));

        let once = one_step.advance(target, Duration::from_millis(32));
        two_steps.advance(target, Duration::from_millis(16));
        let twice = two_steps.advance(target, Duration::from_millis(16));

        for (a, b) in [once.left(), once.top(), once.width(), once.height()]
            .into_iter()
            .zip([twice.left(), twice.top(), twice.width(), twice.height()])
        {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn settles_visually_at_the_configured_time() {
        let target = Rect::new(300.0, 200.0, 800.0, 600.0);
        let mut animation = AnimatedRect::new(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Duration::from_millis(120),
        );

        let rect = animation.advance(target, Duration::from_millis(120));
        let relative_error = (target.left() - rect.left()) / target.left();
        assert!(relative_error.abs() < 0.001);

        animation.advance(target, Duration::from_millis(40));
        assert!(animation.is_settled(target));
    }

    #[test]
    fn spring_settling_checks_position_and_velocity() {
        let target = 100.0;
        assert!(Spring::new(target).is_settled(target));
        assert!(
            !Spring {
                value: target + POSITION_EPSILON * 2.0,
                velocity: 0.0,
            }
            .is_settled(target)
        );
        assert!(
            !Spring {
                value: target,
                velocity: VELOCITY_EPSILON * 2.0,
            }
            .is_settled(target)
        );
    }
}
