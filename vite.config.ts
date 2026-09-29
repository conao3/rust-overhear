import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  build: {
    rollupOptions: {
      // 字幕バーは別ページ。クエリでの振り分けは配布ビルドで壊れる。
      input: {
        main: resolve(__dirname, "index.html"),
        caption: resolve(__dirname, "caption.html"),
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
