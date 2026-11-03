import path from "node:path";

import tailwindcss from "@tailwindcss/vite";
import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import viteReact from "@vitejs/plugin-react";
import { nitro } from "nitro/vite";
import { defineConfig } from "vite";

const src = path.join(import.meta.dirname, "src");

export default defineConfig(({ command }) => ({
  // Bind all interfaces so the site is reachable from other devices on the
  // network (phone, another machine over Tailscale).
  server: { host: true, port: 8080 },
  // Vite resolves tsconfig `paths` and the `@` alias natively, so no
  // vite-tsconfig-paths plugin is needed.
  resolve: {
    alias: { "@": src },
    tsconfigPaths: true,
    dedupe: [
      "react",
      "react-dom",
      "react/jsx-runtime",
      "react/jsx-dev-runtime",
      "@tanstack/react-query",
      "@tanstack/query-core",
    ],
  },
  optimizeDeps: {
    include: ["react", "react-dom", "react-dom/client"],
  },
  environments: {
    ssr: {
      // Workerd cannot execute React's CommonJS entry points directly.
      // Prebundle them as ESM for Nitro's Cloudflare dev runner.
      optimizeDeps: {
        noDiscovery: false,
        include: [
          "react",
          "react/jsx-runtime",
          "react/jsx-dev-runtime",
          "react-dom",
          "react-dom/server",
        ],
      },
    },
  },
  css: { transformer: "lightningcss" },
  plugins: [
    tailwindcss(),
    tanstackStart({
      // Redirect TanStack Start's bundled server entry to src/server.ts (our SSR error wrapper).
      server: { entry: "server" },
      importProtection: {
        behavior: "error",
        client: {
          files: ["**/server/**"],
          specifiers: ["server-only"],
        },
      },
    }),
    viteReact(),
    // Use Node for local dev/preview: the Miniflare runner fails to resolve
    // Nitro's generated worker imports on Windows. Builds target Cloudflare.
    // NITRO_PRESET can still opt into a specific runtime explicitly.
    nitro({
      preset:
        process.env["NITRO_PRESET"] ??
        (command === "serve" ? "node-server" : "cloudflare-module"),
    }),
  ],
}));
