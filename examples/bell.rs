//! 벨 상태(얽힘) 예제: `cargo run --example bell`

use qxide::Circuit;

fn main() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();

    c.h(&mut a); // a를 0과 1의 중첩으로
    c.cx(&mut a, &mut b); // a와 b를 얽음

    let ma = c.measure(a); // 여기서 a, b는 사라지고 고전 비트만 남음
    let mb = c.measure(b);

    println!("--- OpenQASM 3 (실제 양자컴퓨터로 보낼 수 있는 형식) ---");
    print!("{}", c.to_qasm());

    println!("\n--- 시뮬레이터로 1000번 실행 ---");
    let mut counts = [0u32; 4];
    for seed in 0..1000 {
        let shot = c.simulate(seed);
        let k = (shot.get(ma) as usize) << 1 | shot.get(mb) as usize;
        counts[k] += 1;
    }
    for (k, n) in counts.iter().enumerate() {
        println!("{}{}: {n}", k >> 1, k & 1);
    }
    println!("(얽혀 있어서 01, 10은 한 번도 나오지 않아요)");
}
