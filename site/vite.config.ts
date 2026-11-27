import {defineConfig} from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // The playground bundles ../examples/qx at build time; let the dev server read it too, but
  // nothing else outside the site.
  server: {fs: {allow: ['.', '../examples/qx']}},
  worker: {format: 'es'},
});
