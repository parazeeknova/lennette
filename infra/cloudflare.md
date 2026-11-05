# Website deployment

The `lennette` Cloudflare Worker builds from `apps/web`, not the workspace root.
Configure Workers Builds with:

- Production branch: `main`
- Root directory: `apps/web`
- Build command: `bun run build`
- Deploy command: `npx wrangler deploy`

The root `bun run build` also compiles the Rust API and desktop application.
Cloudflare's build image has no Rust toolchain, so that command fails before
building the website. The website directory also contains `wrangler.jsonc` and
Nitro's generated Wrangler deployment redirect.

For local validation, run these commands from `apps/web`:

```sh
bun install --frozen-lockfile
bun run build
bunx --no-install wrangler deploy --dry-run
bunx --no-install wrangler dev --ip 127.0.0.1 --port 18080
```

With the local Worker running, `bun run seo:check http://127.0.0.1:18080`
checks the production SSR output, metadata, sitemap, redirects, and robots rules.
The quality-check workflow runs the same production build and Worker checks.

Account and workspace requests go directly from the browser to
`https://lennapi.itssingularity.com` with credentials. This is the production
default in `apps/web/src/lib/account-api.ts`; no Worker service binding is
required. `VITE_API_URL` is an optional build-time override. The API must allow
the website origin `https://lennette.itssingularity.com` with credentials in its
CORS responses. `/health/ready` returns 204 when its dependencies are ready;
`/v1/me` returns 401 for a signed-out browser.
