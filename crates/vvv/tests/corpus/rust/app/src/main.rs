mod cli;

use corpus_core::geometry::origin;
use corpus_core::{area, Point, Shape};

fn main() {
    let p = Point::new(1, 2);
    let s = Shape::Dot(p);
    println!("{} {} {:?}", s.area(), area(3), origin());
    cli::run();
}
