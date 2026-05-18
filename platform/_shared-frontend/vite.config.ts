import { defineConfig } from "vite";
import { resolve } from "path";

export default defineConfig({
  root: "src",
  base: "./",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        index: resolve(__dirname, "src/index.html"),
        controlPanel: resolve(__dirname, "src/control-panel.html"),
        regionEditor: resolve(__dirname, "src/region-editor.html"),
        renderLayer: resolve(__dirname, "src/render-layer.html"),
        labeling: resolve(__dirname, "src/labeling.html"),
        training: resolve(__dirname, "src/training.html"),
      }
    }
  },
  server: { port: 1420, strictPort: true }
});
