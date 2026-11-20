// Decorative wave paths for the hero background. Period 600 so a 600px translateX loop is seamless.
function wave(y: number, amp: number, period: number, width: number, phase: number): string {
  const pts: string[] = [];
  for (let x = 0; x <= width; x += 20) {
    const yy = y + amp * Math.sin(((x + phase) / period) * Math.PI * 2);
    pts.push(`${x === 0 ? 'M' : 'L'}${x} ${yy.toFixed(1)}`);
  }
  return pts.join(' ');
}

export const WAVE_WIDTH = 3000;
export const WAVES = [
  {d: wave(150, 34, 600, WAVE_WIDTH, 0), cls: 'wave-a'},
  {d: wave(190, 22, 300, WAVE_WIDTH, 90), cls: 'wave-b'},
  {d: wave(120, 14, 600, WAVE_WIDTH, 300), cls: 'wave-c'},
];
