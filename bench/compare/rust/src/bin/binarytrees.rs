// Same algorithm as bench/compare/qx/binarytrees.qx: complete trees of boxed nodes.
use std::hint::black_box;
use std::time::Instant;

enum Tree {
    Leaf,
    Node(Box<Tree>, Box<Tree>),
}

fn make(depth: i64) -> Tree {
    if depth == 0 {
        return Tree::Node(Box::new(Tree::Leaf), Box::new(Tree::Leaf));
    }
    Tree::Node(Box::new(make(depth - 1)), Box::new(make(depth - 1)))
}

fn check(t: &Tree) -> i64 {
    match t {
        Tree::Leaf => 0,
        Tree::Node(l, r) => 1 + check(l) + check(r),
    }
}

fn run(max_depth: i64) -> i64 {
    let min_depth = 4;
    let mut total = check(&make(max_depth + 1));
    let long_lived = make(max_depth);
    let mut d = min_depth;
    while d <= max_depth {
        let iterations = 1i64 << (max_depth - d + min_depth);
        for _ in 0..iterations {
            total += check(&make(d));
        }
        d += 2;
    }
    total + check(&long_lived)
}

fn main() {
    let depth: i64 = std::env::args()
        .nth(1)
        .expect("DEPTH")
        .parse()
        .expect("int");
    let t0 = Instant::now();
    let total = black_box(run(black_box(depth)));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("result={total} ms={ms:.3}");
}
