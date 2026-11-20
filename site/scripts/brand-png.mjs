// Renders assets/brand/*.svg to PNG with sharp (not a saved dependency: `npm install --no-save sharp` first).
// Usage, from site/: node scripts/brand-png.mjs
import {mkdirSync, readFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import sharp from 'sharp';

const brand = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'assets', 'brand');
const out = join(brand, 'png');
mkdirSync(out, {recursive: true});

const jobs = [
  ['qxide-icon.svg', 'qxide-icon', [512, 192, 180, 32, 16]],
  ['qxide-mark.svg', 'qxide-mark', [512]],
];

for (const [file, name, sizes] of jobs) {
  const svg = readFileSync(join(brand, file));
  for (const size of sizes) {
    // High density keeps small sizes crisp; the SVG viewBox is 100 units.
    await sharp(svg, {density: Math.max(72, (size / 100) * 72 * 2)})
      .resize(size, size)
      .png({compressionLevel: 9})
      .toFile(join(out, `${name}-${size}.png`));
  }
}
console.log('done');
