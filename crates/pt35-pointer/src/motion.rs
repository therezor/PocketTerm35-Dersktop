//! Turning held direction keys into pointer motion.
//!
//! A D-pad has no analogue deflection, so the feel comes entirely from
//! acceleration: a tap nudges by a pixel or two, holding a direction ramps up
//! to something that crosses 640 px in well under a second.

use pt35_common::theme::Pointer as PointerTheme;
use pt35_ui::keys::{sym, Key};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn from_sym(value: u32) -> Option<Self> {
        match value {
            sym::UP => Some(Direction::Up),
            sym::DOWN => Some(Direction::Down),
            sym::LEFT => Some(Direction::Left),
            sym::RIGHT => Some(Direction::Right),
            _ => None,
        }
    }

    fn delta(self) -> (f32, f32) {
        match self {
            Direction::Up => (0.0, -1.0),
            Direction::Down => (0.0, 1.0),
            Direction::Left => (-1.0, 0.0),
            Direction::Right => (1.0, 0.0),
        }
    }
}

/// Which directions are held, and for how long.
#[derive(Debug, Default)]
pub struct Motion {
    held: Vec<Direction>,
    /// Seconds the current run of movement has lasted.
    elapsed: f32,
}

impl Motion {
    pub fn press(&mut self, direction: Direction) {
        if !self.held.contains(&direction) {
            self.held.push(direction);
        }
    }

    pub fn release(&mut self, direction: Direction) {
        self.held.retain(|d| *d != direction);
        if self.held.is_empty() {
            self.elapsed = 0.0;
        }
    }

    pub fn moving(&self) -> bool {
        !self.held.is_empty()
    }

    /// Advance by one frame and return the (dx, dy) to send, in pixels.
    pub fn step(&mut self, theme: &PointerTheme, frame: Duration) -> (f32, f32) {
        if self.held.is_empty() {
            return (0.0, 0.0);
        }
        let dt = frame.as_secs_f32();
        self.elapsed += dt;

        // Ramp from a quarter speed up to full over the first second, so a tap
        // is precise and a hold is fast.
        let ramp = (0.25 + self.elapsed.min(1.0).powf(theme.accel)).min(1.0);
        let speed = theme.speed * ramp * dt;

        let (mut dx, mut dy) = (0.0, 0.0);
        for direction in &self.held {
            let (x, y) = direction.delta();
            dx += x;
            dy += y;
        }
        // Diagonals should not be faster than the cardinals.
        let length = (dx * dx + dy * dy).sqrt();
        if length > 0.0 {
            dx = dx / length * speed;
            dy = dy / length * speed;
        }
        (dx, dy)
    }
}

/// Buttons, as evdev codes — what the virtual-pointer protocol expects.
pub mod button {
    pub const LEFT: u32 = 0x110;
    pub const RIGHT: u32 = 0x111;
    pub const MIDDLE: u32 = 0x112;
}

/// Everything the pointer does besides moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Click(u32),
    ScrollUp,
    ScrollDown,
    /// Enter grid-jump mode.
    Grid,
    Quit,
}

/// Map a key press to an action. Movement keys are handled separately.
pub fn action(key: &Key) -> Option<Action> {
    match key.sym {
        sym::RETURN | sym::KP_ENTER | sym::SPACE => Some(Action::Click(button::LEFT)),
        sym::ESCAPE => Some(Action::Quit),
        sym::PAGE_UP => Some(Action::ScrollUp),
        sym::PAGE_DOWN => Some(Action::ScrollDown),
        _ => match key.text {
            Some('g') => Some(Action::Grid),
            Some('r') => Some(Action::Click(button::RIGHT)),
            Some('m') => Some(Action::Click(button::MIDDLE)),
            Some('q') => Some(Action::Quit),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> PointerTheme {
        PointerTheme {
            speed: 400.0,
            accel: 2.0,
            ..PointerTheme::default()
        }
    }

    const FRAME: Duration = Duration::from_millis(16);

    #[test]
    fn nothing_held_means_no_motion() {
        let mut motion = Motion::default();
        assert_eq!(motion.step(&theme(), FRAME), (0.0, 0.0));
        assert!(!motion.moving());
    }

    #[test]
    fn a_tap_moves_a_little_and_a_hold_accelerates() {
        let mut motion = Motion::default();
        motion.press(Direction::Right);
        let (first, _) = motion.step(&theme(), FRAME);
        let mut last = first;
        for _ in 0..60 {
            last = motion.step(&theme(), FRAME).0;
        }
        assert!(
            first > 0.0 && first < 3.0,
            "a single frame should be a nudge: {first}"
        );
        assert!(
            last > first * 2.0,
            "holding must accelerate: {first} -> {last}"
        );
    }

    #[test]
    fn diagonals_are_not_faster_than_cardinals() {
        let mut straight = Motion::default();
        straight.press(Direction::Right);
        let (dx, _) = straight.step(&theme(), FRAME);

        let mut diagonal = Motion::default();
        diagonal.press(Direction::Right);
        diagonal.press(Direction::Down);
        let (ddx, ddy) = diagonal.step(&theme(), FRAME);
        let length = (ddx * ddx + ddy * ddy).sqrt();
        assert!(
            (length - dx).abs() < 0.01,
            "diagonal speed {length} != cardinal {dx}"
        );
    }

    #[test]
    fn releasing_resets_the_ramp() {
        let mut motion = Motion::default();
        motion.press(Direction::Left);
        for _ in 0..60 {
            motion.step(&theme(), FRAME);
        }
        motion.release(Direction::Left);
        assert!(!motion.moving());
        motion.press(Direction::Left);
        let (dx, _) = motion.step(&theme(), FRAME);
        assert!(dx.abs() < 3.0, "a fresh press starts slow again: {dx}");
    }

    #[test]
    fn maps_keys_to_clicks_and_modes() {
        assert_eq!(
            action(&Key::new(sym::RETURN)),
            Some(Action::Click(button::LEFT))
        );
        assert_eq!(
            action(&Key::with_text(0x0072, 'r')),
            Some(Action::Click(button::RIGHT))
        );
        assert_eq!(action(&Key::with_text(0x0067, 'g')), Some(Action::Grid));
        assert_eq!(action(&Key::new(sym::ESCAPE)), Some(Action::Quit));
        assert_eq!(action(&Key::new(sym::PAGE_UP)), Some(Action::ScrollUp));
        assert_eq!(action(&Key::new(sym::TAB)), None);
    }
}
