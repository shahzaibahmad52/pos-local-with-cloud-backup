import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// In the browser during development, /api is forwarded to a store server
// started with `npm run server:demo`. The desktop app calls the server directly.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: { '/api': 'http://127.0.0.1:7420' },
  },
  build: { target: 'es2022' },
  test: { environment: 'node' },
});
