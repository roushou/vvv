use super::Point;
use crate::util::area;

pub enum Shape {
    Dot(Point),
    Square { corner: Point, side: u32 },
}

impl Shape {
    pub fn area(&self) -> u32 {
        match self {
            Shape::Dot(_) => 0,
            Shape::Square { side, .. } => area(*side),
        }
    }

    pub fn anchor(&self) -> Point {
        match self {
            Shape::Dot(p) | Shape::Square { corner: p, .. } => *p,
        }
    }
}
