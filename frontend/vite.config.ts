import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";
import path from "path";

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: "autoUpdate",
      manifest: false,
      workbox: {
        globPatterns: ["**/*.{js,css,html,ico,svg,png,woff,woff2}"],
        navigateFallback: "index.html",
        navigateFallbackDenylist: [/^\/v1\//],
        runtimeCaching: [
          {
            urlPattern: /\.(?:png|jpg|jpeg|svg|gif|webp|ico|woff|woff2|ttf|eot)$/i,
            handler: "CacheFirst",
            options: {
              cacheName: "static-assets",
              expiration: {
                maxEntries: 100,
                maxAgeSeconds: 30 * 24 * 60 * 60, // 30 days
              },
            },
          },
          {
            urlPattern: /^(?!.*[?&]_sw-bypass=).*\/v1\/.*$/i,
            method: "GET",
            handler: "NetworkFirst",
            options: {
              cacheName: "api-cache",
              expiration: {
                maxEntries: 50,
                maxAgeSeconds: 24 * 60 * 60, // 24 hours
              },
              networkTimeoutSeconds: 5,
              cacheableResponse: {
                statuses: [0, 200],
              },
            },
          },
        ],
      },
    }),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    proxy: {
      "/v1": {
        target: "http://localhost:3000",
        changeOrigin: true,
      },
      "/auth/login": {
        target: "http://localhost:3000",
        changeOrigin: true,
      },
      "/auth/logout": {
        target: "http://localhost:3000",
        changeOrigin: true,
      },
      // Proxy /auth/callback only for API fetch calls (not browser navigation).
      // Browser navigations request text/html; fetch calls request application/json.
      "/auth/callback": {
        target: "http://localhost:3000",
        changeOrigin: true,
        bypass(req) {
          const accept = req.headers["accept"] ?? "";
          if (accept.includes("text/html")) {
            // Return index.html for browser navigation (SPA handles the route)
            return "/index.html";
          }
          // Otherwise proxy to backend (API fetch calls)
        },
      },
      "/me": {
        target: "http://localhost:3000",
        changeOrigin: true,
      },
    },
  },
});
