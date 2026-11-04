import { statSync } from "node:fs";
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
  server: { host: true, port: 8080, strictPort: true },
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
    {
      name: "lennette-dev-lifecycle",
      configureServer(server) {
        const lease = process.env["LENNETTE_DEV_LEASE"];
        if (!lease) {
          return;
        }
        const timer = setInterval(() => {
          let alive = false;
          try {
            alive = Date.now() - statSync(lease).mtimeMs < 8000;
          } catch {
            /* Supervisor exited. */
          }
          if (!alive) {
            clearInterval(timer);
            void server.close().then(() => process.exit(0));
          }
        }, 500);
        timer.unref();
        server.httpServer?.once("close", () => clearInterval(timer));
      },
    },
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
