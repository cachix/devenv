use std::{fmt, time::Duration};

pub struct HumanReadableDuration(pub Duration);

impl std::fmt::Display for HumanReadableDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut t = self.0.as_nanos() as f64;
        for unit in ["ns", "µs", "ms", "s"].iter() {
            if t < 10.0 {
                return write!(f, "{t:.2}{unit}");
            } else if t < 100.0 {
                return write!(f, "{t:.1}{unit}");
            } else if t < 1000.0 {
                return write!(f, "{t:.0}{unit}");
            }
            t /= 1000.0;
        }
        write!(f, "{:.0}s", t * 1000.0)
    }
}


