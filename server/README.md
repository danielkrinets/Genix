# Genix licence server

A Cloudflare Worker (no database) between the app and Stripe. Subscription: **USD 0.99 per month**, 7-day trial in the app.

| Route | What it does |
|---|---|
| `GET /buy` | creates a Stripe Checkout subscription session and redirects to it |
| `GET /success?session_id=…` | after payment: shows the signed key to paste into the app |
| `POST /refresh` `{key}` | gives a fresh key while the Stripe subscription is active (the app calls it itself in the last 5 days) |
| `GET /manage?key=…` | opens the Stripe customer portal (change card, cancel) |
| `POST /webhook` | Stripe events, signature-checked (nothing is stored; keys come from Stripe's current state) |

A key is `base64url({email,cid,until}).base64url(Ed25519 signature)`. The app checks it offline against the public key in
`crates/app/src/license.rs`. `until` = end of the paid period + 3 grace days. The private key never goes in git.

## Set-up (you run these; secrets are never typed by the assistant)

1. In Stripe (test mode first): Products → add «Genix Pro», recurring, 0.99 USD / month → copy the `price_…` id into `wrangler.toml` (`STRIPE_PRICE_ID`).
   Settings → Billing → Customer portal → switch it on (allow cancelling).
2. `cd server && npm i -g wrangler` (or `npx wrangler`), then `wrangler login`.
3. Secrets (each command asks for the value):
   ```bash
   wrangler secret put STRIPE_SECRET_KEY
   wrangler secret put STRIPE_WEBHOOK_SECRET
   base64 -i ~/Desktop/Genix-keys/license-private.pk8 | tr -d '\n' | wrangler secret put LICENSE_PRIVATE_KEY
   ```
4. `wrangler deploy`, attach the domain (e.g. `api.genix.app`) and put the same address in `API_URL` in `license.rs`.
5. Stripe → Developers → Webhooks → add `https://api.genix.app/webhook` (events `customer.subscription.*`, `invoice.*`); its signing secret is `STRIPE_WEBHOOK_SECRET`.
6. Test with card `4242 4242 4242 4242`. Local: `wrangler dev`, then run the app with `GENIX_LICENSE_API=http://localhost:8787`.
7. Switch Stripe to live mode, replace `sk_live…` and the live `price_…`.

Tests: `cd server && npm test` (Node ≥ 20, Stripe is faked).
