//! UI-independent window and region selection.
//!
//! [`Selector`] can be driven by a custom frontend instead of the built-in
//! [`crate::overlay`] UI. Feed it pointer movement, press, and release events,
//! then render [`Selector::target_geometry`] or [`Selector::region`].

use crate::{Point, Rect, Scene};

#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    Window(Rect),
    Region(Rect),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectionResult {
    Selected(Selection),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
enum State {
    Waiting,
    Hover {
        pointer: Point,
        target: Option<usize>,
    },
    Pressed {
        origin: Point,
        target: Option<usize>,
    },
    Dragging {
        origin: Point,
        pointer: Point,
    },
}

/// Selection state machine for pointer-driven frontends.
pub struct Selector {
    scene: Scene,
    state: State,
    drag_threshold_squared: f64,
}

impl Selector {
    /// Creates a selector over `scene`.
    ///
    /// Pointer movement must reach `drag_threshold` logical pixels after a
    /// press before the interaction becomes a region drag.
    #[must_use]
    pub fn new(scene: Scene, drag_threshold: f64) -> Self {
        debug_assert!(drag_threshold >= 0.0);

        Self {
            scene,
            state: State::Waiting,
            drag_threshold_squared: drag_threshold.powi(2),
        }
    }

    #[must_use]
    pub const fn scene(&self) -> &Scene {
        &self.scene
    }

    #[must_use]
    pub fn target_geometry(&self) -> Option<Rect> {
        let target = match self.state {
            State::Hover { target, .. } | State::Pressed { target, .. } => target,
            State::Waiting | State::Dragging { .. } => None,
        }?;

        Some(self.scene.windows[target].geometry)
    }

    #[must_use]
    pub fn region(&self) -> Option<Rect> {
        let State::Dragging { origin, pointer } = self.state else {
            return None;
        };

        Some(Rect::from_points(origin, pointer))
    }

    pub fn pointer_moved(&mut self, pointer: Point) {
        self.state = match self.state {
            State::Waiting | State::Hover { .. } => State::Hover {
                pointer,
                target: self.scene.window_index_at(pointer),
            },
            State::Pressed { origin, .. }
                if origin.distance_squared(pointer) >= self.drag_threshold_squared =>
            {
                State::Dragging { origin, pointer }
            }
            State::Pressed { origin, target } => State::Pressed { origin, target },
            State::Dragging { origin, .. } => State::Dragging { origin, pointer },
        };
    }

    pub const fn press(&mut self) {
        if let State::Hover { pointer, target } = self.state {
            self.state = State::Pressed {
                origin: pointer,
                target,
            };
        }
    }

    pub fn release(&mut self) -> Option<SelectionResult> {
        let result = match self.state {
            State::Pressed {
                target: Some(target),
                ..
            } => Selection::Window(self.scene.windows[target].geometry),
            State::Dragging { origin, pointer } => {
                let region = Rect::from_points(origin, pointer);
                if region.width() == 0.0 || region.height() == 0.0 {
                    self.state = State::Hover {
                        pointer,
                        target: self.scene.window_index_at(pointer),
                    };

                    return None;
                }

                Selection::Region(region)
            }
            State::Pressed {
                origin,
                target: None,
            } => {
                self.state = State::Hover {
                    pointer: origin,
                    target: None,
                };

                return None;
            }
            State::Waiting | State::Hover { .. } => return None,
        };

        Some(SelectionResult::Selected(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{output, window};

    fn scene() -> Scene {
        Scene {
            outputs: vec![output("DP-1", 1.0)],
            windows: vec![window(Rect::new(100.0, 100.0, 800.0, 600.0))],
        }
    }

    fn selector() -> Selector {
        Selector::new(scene(), 4.0)
    }

    #[test]
    fn click_selects_the_snapped_window() {
        let mut selector = selector();

        selector.pointer_moved(Point::new(200.0, 200.0));
        selector.press();
        assert_eq!(
            selector.release(),
            Some(SelectionResult::Selected(Selection::Window(Rect::new(
                100.0, 100.0, 800.0, 600.0
            ))))
        );
    }

    #[test]
    fn dragging_past_the_threshold_selects_a_region() {
        let mut selector = selector();

        selector.pointer_moved(Point::new(200.0, 200.0));
        selector.press();
        selector.pointer_moved(Point::new(202.0, 203.0));
        assert_eq!(selector.region(), None);
        selector.pointer_moved(Point::new(150.0, 120.0));
        assert_eq!(
            selector.release(),
            Some(SelectionResult::Selected(Selection::Region(Rect::new(
                150.0, 120.0, 50.0, 80.0
            ))))
        );
    }

    #[test]
    fn releasing_an_empty_click_does_not_start_a_late_drag() {
        let mut selector = selector();

        selector.pointer_moved(Point::new(1_000.0, 1_000.0));
        selector.press();
        assert_eq!(selector.release(), None);
        selector.pointer_moved(Point::new(1_100.0, 1_000.0));

        assert_eq!(selector.region(), None);
    }

    #[test]
    fn a_line_is_not_a_region() {
        let mut selector = selector();

        selector.pointer_moved(Point::new(1_000.0, 1_000.0));
        selector.press();
        selector.pointer_moved(Point::new(1_100.0, 1_000.0));

        assert_eq!(selector.release(), None);
        assert_eq!(selector.region(), None);
    }

    #[test]
    fn releasing_a_line_restores_the_window_target() {
        let mut selector = selector();

        selector.pointer_moved(Point::new(200.0, 200.0));
        selector.press();
        selector.pointer_moved(Point::new(300.0, 200.0));
        assert_eq!(selector.target_geometry(), None);

        assert_eq!(selector.release(), None);
        assert_eq!(
            selector.target_geometry(),
            Some(Rect::new(100.0, 100.0, 800.0, 600.0))
        );
    }
}
