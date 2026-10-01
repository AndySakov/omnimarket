import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    globals: false,
    setupFiles: './src/test/setup.ts',
    include: ['src/**/*.test.{ts,tsx}'],
    // CI's criteria job reads which tests passed from this report (D95).
    reporters: process.env.CI ? ['default', 'json'] : ['default'],
    outputFile: { json: 'test-reports/vitest.json' },
  },
})
