// Same algorithm as bench/compare/qx/strings.qx: build words, join, split, count in a map.
use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

fn process(words: i64) -> i64 {
    let letters = "abcdefghijklmnop";
    let mut parts: Vec<String> = Vec::new();
    let mut seed: i64 = 12345;
    for _ in 0..words {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        let size = 1 + seed % 6;
        let mut word = String::new();
        for _ in 0..size {
            seed = (seed * 1103515245 + 12345) % 2147483648;
            let c = (seed % 16) as usize;
            word = word + &letters[c..c + 1];
        }
        parts.push(word);
    }
    let text = parts.join(" ");
    let mut counts: HashMap<String, i64> = HashMap::new();
    for word in text.split(' ') {
        *counts.entry(word.to_string()).or_insert(0) += 1;
    }
    let mut sum = 0;
    for (key, n) in &counts {
        sum += n * key.len() as i64 + 1;
    }
    sum
}

fn main() {
    let words: i64 = std::env::args()
        .nth(1)
        .expect("WORDS")
        .parse()
        .expect("int");
    let t0 = Instant::now();
    let sum = black_box(process(black_box(words)));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("result={sum} ms={ms:.3}");
}
