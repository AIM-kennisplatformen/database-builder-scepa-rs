import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  base: "/upload/",
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/upload/api": {
        target: process.env.VITE_API_PROXY_TARGET || "http://localhost:3000",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/upload\/api/, ""),
      },
    },
  },
});
