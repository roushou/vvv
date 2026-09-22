pub mod shape;
mod point;

pub use point::Point;

pub fn origin() -> Point {
    Point::new(0, 0)
}
