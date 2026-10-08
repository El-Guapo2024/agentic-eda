use eda_model::ir::Point;
use eda_pns::hull::segment_hull;
#[test]
fn hull_probe() {
    let a = Point { x: 221200, y: 50800 };
    let b = Point { x: 224600, y: 54200 };
    let h = segment_hull(a, b, 500, 251, 500);
    println!("diag hull {:?}", h);
    let h2 = segment_hull(Point{x:215265,y:50800}, a, 500, 251, 500);
    println!("horiz hull {:?}", h2);
}
