use corpus_core::geometry::shape::Shape as Figure;
use corpus_core::Hidden;

pub fn run() {
    let hidden = Hidden;
    let figure = Figure::Dot(corpus_core::geometry::origin());
    println!("{} {}", hidden.reveal(), figure.area());
}
