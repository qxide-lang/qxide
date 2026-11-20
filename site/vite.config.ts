import {defineConfig} from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // The playground bundles ../examples/qx at build time; let the dev server read it too.
  server: {fs: {allow: ['..']}},
  worker: {format: 'es'},
});
