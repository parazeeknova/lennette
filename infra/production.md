# Lennette production

The API endpoint is `https://lennapi.itssingularity.com`, with the website at
`https://lennette.itssingularity.com`. Cloudflare DNS points the API at Dokploy.
Runtime credentials live in Dokploy, not in the repository or image.

## Services

| Service | Production configuration |
| --- | --- |
| Dokploy | General project, `lennette` environment, `Lennette API` Docker application |
| API application | `QCdzh_QeHNuc0tQANc9Bc`, port 8090, one ARM64 replica |
| Neon | Existing `Lennette` project `curly-haze-55537263`, `production` branch |
| Upstash | Native Redis protocol over TLS, dedicated `lennette_api_production` ACL credential |
| ClamAV | General project, `security` environment, `Shared ClamAV` compose service |
| OpenObserve | Existing Observability Stack in Analytics; separate `Lennette` organization |
| Dodo Payments | Verified `Lennette` brand under Singularity Works, live mode |

ClamAV retains its original compose identity and `asm-clamav-data` volume.
`asm-clamav:3310` remains reachable over `dokploy-network` from both Lennette and
asocialmedia; the existing asocialmedia environment remains valid after the move.
ClamAV has no public domain or exposed host port.

## Runtime settings

Required infrastructure settings are `DATABASE_URL` (Neon pooled),
`DATABASE_URL_UNPOOLED` (direct, for migrations and `LISTEN`), `REDIS_URL`
(`rediss://`), and `CLAMD_ADDRESS=asm-clamav:3310`.
The deployed PostgreSQL URLs use `sslmode=verify-full` for certificate and
hostname verification. SQLx does not implement libpq's `channel_binding` option;
it is omitted from the deployed URLs rather than advertised as enforced.

Set `API_BIND=0.0.0.0:8090`, `API_PUBLIC_URL=https://lennapi.itssingularity.com`, and
`WEBSITE_URL=https://lennette.itssingularity.com`. Register Google and GitHub OAuth
callbacks at `/v1/auth/google/callback` and `/v1/auth/github/callback` on the API
domain. Their client IDs and secrets are configured in Dokploy.

R2 uses the existing private `lennette` bucket with `R2_ENDPOINT`, `R2_BUCKET`,
`R2_ACCESS_KEY_ID`, and `R2_SECRET_ACCESS_KEY`. Managed providers use
`MERGE_API_KEY` and `DEEPGRAM_API_KEY`.

OpenObserve uses `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`,
`OTEL_EXPORTER_OTLP_ENDPOINT=https://o11y.asocialmedia.cc/api/3LgChM2bGiw3IdbWav8jMqFn3TW`,
and a dedicated ingestion token encoded in `OTEL_EXPORTER_OTLP_HEADERS`, with
`stream-name=lennette_api`. The token cannot list or read streams. Resource
attributes identify `deployment.environment.name=production` and
`service.namespace=lennette`. Logs, traces, HTTP metrics, database metrics, Redis
metrics, and cleanup metrics use this existing stack.

Do not trust `CF-Connecting-IP` from arbitrary direct-origin requests.
`TRUSTED_PROXY_IPS` stays unset until the origin route enforces Cloudflare source
addresses and the trusted proxy addresses can be maintained reliably. While unset,
clients through the ingress share its socket-peer budgets: 30 `/auth/` requests
and 1200 other API requests per 60 seconds, returning HTTP 429 above the limit.
Paths containing `/auth/` or `/browser/` also share a global budget of 500 requests
per 60 seconds; setting `TRUSTED_PROXY_IPS` does not change that global budget.

## Live billing

Products are monthly USD SaaS subscriptions without a trial. Applicable taxes
are added at checkout. Their total subscription duration is Dodo's recommended
20 years, so monthly billing continues until cancellation rather than expiring
after one month.

| Plan | Price | Live product ID | Application plan ID |
| --- | --- | --- | --- |
| Managed | $17.10/month | `pdt_0NrHBaneSl8JBiix8G4Fv` | `managed` |
| Managed Plus | $40/month | `pdt_0NrHBapXY6L5g2FaQQPrm` | `managed_plus` |

`DODO_API_URL=https://live.dodopayments.com/`. The live API key, webhook secret,
`DODO_STANDARD_PRODUCT_ID`, and `DODO_PLUS_PRODUCT_ID` are configured in Dokploy.
The webhook is `https://lennapi.itssingularity.com/v1/billing/webhook`, subscribes
to subscription lifecycle changes, successful refunds and disputes, and verifies
Standard Webhooks signatures before processing events. Tests use isolated
accounts and mock billable provider responses; they do not charge a live card.

## Verification and development

`/health/live` and `/health/ready` return HTTP 204. Readiness queries the migrated
tables and pings Redis. The container health check uses readiness; updates use
stop-first ordering and rollback on failure. Memory is limited to 1 GiB and CPU
to one core, with a 256 MiB memory reservation.

```sh
npm run build
bun test scripts/dev-env.test.ts scripts/deploy-api.test.ts
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked -p lennette-server --test integration -- --ignored
```

For cloud integration tests, set `TEST_DATABASE_URL` to a disposable Neon branch,
`TEST_DATABASE_URL_UNPOOLED` to that branch's direct URL, and `TEST_REDIS_URL` to
the TLS Redis URL. Never run fixture tests against production PostgreSQL.
The temporary validation branch was deleted after cloud integration tests passed.

Neon setup installs its Codex plugin and adds `neon.ts`; `.neon` and `.env.local`
remain ignored. `neon deploy` reconciles PostgreSQL configuration and pulls its
environment variables. Development commands use `DEV_DATABASE_URL`,
`DEV_DATABASE_URL_UNPOOLED`, and `DEV_REDIS_URL` so a production environment pull
cannot redirect local traffic or migrations into production.

The Upstash Codex plugin bundles its MCP server and skills. Restart the agent to
load newly installed plugins. Account-management MCP access uses OAuth; the
running backend authenticates with its own Redis credential independently.
