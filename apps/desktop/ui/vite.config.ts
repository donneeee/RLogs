import { defineConfig } from "vitest/config";

export default defineConfig({
  build: {
    target: "es2022",
    sourcemap: true,
  },
  test: {
    include: ["src/**/*.test.ts"],
  },
});
