//! Multi-touch pinch / rotate gestures (Android).
//!
//! `TouchPoint`s flow through [`AndroidWindow::handle_touch`](super::window::AndroidWindow::handle_touch)
//! one pointer at a time; this module tracks the active pointer set and
//! derives classic two-finger transforms from it:
//!
//! - **pinch** — multiplicative span change since the last drain
//!   ([`take_scale`]); apps call it from `render` to zoom continuously.
//! - **twist** — signed angle change in degrees
//!   ([`take_rotation_delta`]).
//!
//! Single-pointer mouse translation (in `window.rs`) is suppressed while two
//! or more fingers are down ([`is_pinching`]) so the extra pointers don't
//! emit spurious taps or drags.
//!
//! State is process-wide: Android apps have exactly one window, and touch
//! events all arrive on the same input thread. The drain (take) APIs are
//! meant to be called from the main thread's render pass.

use std::sync::Mutex;

use super::TouchPoint;

/// Two-finger gesture state, in physical (device) pixels.
struct GestureState {
    /// Active pointers: `(id, x, y)` physical.
    pointers: Vec<(i32, f32, f32)>,
    /// `(span, angle_rad)` baseline between the first two pointers.
    baseline: Option<(f32, f32)>,
    /// Multiplicative zoom accumulated since the last [`take_scale`].
    scale: f32,
    /// Signed rotation accumulated since the last [`take_rotation_delta`].
    rot_deg: f32,
    /// Centroid of the two pointers (physical px), for callers that want to
    /// zoom around the pinch centre.
    centroid: (f32, f32),
}

static STATE: Mutex<GestureState> = Mutex::new(GestureState {
    pointers: Vec::new(),
    baseline: None,
    scale: 1.0,
    rot_deg: 0.0,
    centroid: (0.0, 0.0),
});

/// Feed one touch point; returns whether a two-finger gesture is active.
///
/// `point.action` uses the standard `AMOTION_EVENT_ACTION_*` codes, including
/// `ACTION_POINTER_DOWN` (5) / `ACTION_POINTER_UP` (6) for additional fingers.
pub fn track_touch(point: &TouchPoint) -> bool {
    let mut st = STATE.lock().unwrap();
    match point.action {
        0 => {
            // Fresh gesture — ACTION_DOWN is only sent for the first finger.
            st.pointers.clear();
            st.pointers.push((point.id, point.x, point.y));
            st.baseline = None;
        }
        5 => {
            // ACTION_POINTER_DOWN — an additional finger landed.
            if let Some(p) = st.pointers.iter_mut().find(|p| p.0 == point.id) {
                *p = (point.id, point.x, point.y);
            } else {
                st.pointers.push((point.id, point.x, point.y));
            }
            st.baseline = None; // (re)baseline when the gesture (re)starts
        }
        2 => {
            if let Some(p) = st.pointers.iter_mut().find(|p| p.0 == point.id) {
                *p = (point.id, point.x, point.y);
            }
        }
        1 | 3 | 6 => {
            st.pointers.retain(|p| p.0 != point.id);
            st.baseline = None;
        }
        _ => {}
    }

    if st.pointers.len() >= 2 {
        let (ax, ay) = (st.pointers[0].1, st.pointers[0].2);
        let (bx, by) = (st.pointers[1].1, st.pointers[1].2);
        let span = (bx - ax).hypot(by - ay).max(1.0);
        let angle = (by - ay).atan2(bx - ax);
        st.centroid = ((ax + bx) * 0.5, (ay + by) * 0.5);
        if let Some((prev_span, prev_angle)) = st.baseline {
            st.scale = (st.scale * span / prev_span).clamp(0.05, 20.0);
            let mut d = angle - prev_angle;
            // Unwrap the wrap-around so a twist never "jumps" by ±360°.
            while d > std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            }
            while d < -std::f32::consts::PI {
                d += std::f32::consts::TAU;
            }
            st.rot_deg += d.to_degrees();
        }
        st.baseline = Some((span, angle));
        true
    } else {
        false
    }
}

/// Whether two or more fingers are currently down (single-pointer mouse
/// translation should be suppressed).
pub fn is_pinching() -> bool {
    STATE.lock().unwrap().pointers.len() >= 2
}

/// Snapshot of every active pointer, `(id, x, y)` in physical pixels.
///
/// This is the raw multi-touch feed: one entry per finger currently down,
/// kept up to date as fingers move (entries appear on `ACTION_DOWN` /
/// `ACTION_POINTER_DOWN`, update on move, disappear on up/cancel). Apps can
/// hit-test or draw against it directly — multi-touch test screens,
/// multi-player tap zones, virtual joysticks, ...
pub fn pointers() -> Vec<(i32, f32, f32)> {
    STATE.lock().unwrap().pointers.clone()
}

/// Centroid of the two active pointers (physical px).
pub fn centroid() -> (f32, f32) {
    STATE.lock().unwrap().centroid
}

/// Drain the accumulated multiplicative pinch zoom (1.0 = no change).
pub fn take_scale() -> f32 {
    std::mem::replace(&mut STATE.lock().unwrap().scale, 1.0)
}

/// Drain the accumulated signed twist in degrees.
pub fn take_rotation_delta() -> f32 {
    std::mem::replace(&mut STATE.lock().unwrap().rot_deg, 0.0)
}
