// Genix licence server: a Cloudflare Worker between the app and Stripe. It holds no database. A key is an Ed25519-signed
// token `base64url(payload).base64url(signature)` with { email, cid (Stripe customer), until (unix s) }; the app checks it
// offline against the public key built into the binary. Stripe stays the single source of truth for who has paid.
//
// Secrets (wrangler secret put): STRIPE_SECRET_KEY, STRIPE_WEBHOOK_SECRET, LICENSE_PRIVATE_KEY (PKCS#8, base64).
// Variables (wrangler.toml): STRIPE_PRICE_ID, PUBLIC_BASE_URL, LICENSE_PUBLIC_KEY (hex, 32 bytes), GRACE_DAYS (optional).

const DAY = 86400;
const enc = new TextEncoder();

export const b64u = {
  encode(bytes) {
    let s = '';
    for (const b of new Uint8Array(bytes)) s += String.fromCharCode(b);
    return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  },
  decode(text) {
    const s = text.replace(/-/g, '+').replace(/_/g, '/');
    const bin = atob(s + '='.repeat((4 - (s.length % 4)) % 4));
    return Uint8Array.from(bin, (c) => c.charCodeAt(0));
  },
};

const fromHex = (h) => Uint8Array.from(h.match(/../g) ?? [], (x) => parseInt(x, 16));
const fromB64 = (t) => Uint8Array.from(atob(t.trim()), (c) => c.charCodeAt(0));

export async function signKey(env, claims) {
  const key = await crypto.subtle.importKey('pkcs8', fromB64(env.LICENSE_PRIVATE_KEY), 'Ed25519', false, ['sign']);
  const payload = b64u.encode(enc.encode(JSON.stringify(claims)));
  const sig = await crypto.subtle.sign('Ed25519', key, enc.encode(payload));
  return `${payload}.${b64u.encode(sig)}`;
}

/** Claims of a correctly signed key (expired ones too), or null. */
export async function readKey(env, token) {
  try {
    const [payload, sig] = String(token).trim().split('.');
    if (!payload || !sig) return null;
    const key = await crypto.subtle.importKey('raw', fromHex(env.LICENSE_PUBLIC_KEY), 'Ed25519', false, ['verify']);
    const ok = await crypto.subtle.verify('Ed25519', key, b64u.decode(sig), enc.encode(payload));
    if (!ok) return null;
    return JSON.parse(new TextDecoder().decode(b64u.decode(payload)));
  } catch {
    return null;
  }
}

async function stripe(env, method, path, form) {
  const res = await fetch(`https://api.stripe.com/v1${path}`, {
    method,
    headers: { Authorization: `Bearer ${env.STRIPE_SECRET_KEY}`, 'Content-Type': 'application/x-www-form-urlencoded' },
    body: form ? new URLSearchParams(form).toString() : undefined,
  });
  const data = await res.json();
  if (!res.ok) throw new Error(data?.error?.message ?? `Stripe HTTP ${res.status}`);
  return data;
}

/** Stripe moved `current_period_end` from the subscription to its items in newer API versions; accept both. */
export function periodEnd(sub) {
  return sub.current_period_end ?? sub.items?.data?.[0]?.current_period_end ?? 0;
}

const PAID = new Set(['active', 'trialing']);

export async function keyForSubscription(env, sub, email, cid, now) {
  const grace = Number(env.GRACE_DAYS ?? 3) * DAY;
  const until = Math.max(periodEnd(sub), now) + grace;
  return signKey(env, { email, cid, until });
}

const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' } });
const html = (body, status = 200) => new Response(body, { status, headers: { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' } });
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

function page(title, inner) {
  return html(`<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${esc(title)}</title>
<style>body{font:16px/1.5 -apple-system,system-ui,sans-serif;max-width:560px;margin:12vh auto;padding:0 20px;color:#161a24}
textarea{width:100%;height:120px;font:13px ui-monospace,monospace;padding:10px;border-radius:12px;border:1px solid #ccd}
button{font:inherit;padding:10px 18px;border-radius:12px;border:0;background:#2f6bff;color:#fff;cursor:pointer}</style>${inner}`);
}

export async function buy(env, url) {
  const base = env.PUBLIC_BASE_URL || url.origin;
  const session = await stripe(env, 'POST', '/checkout/sessions', {
    mode: 'subscription',
    'line_items[0][price]': env.STRIPE_PRICE_ID,
    'line_items[0][quantity]': '1',
    success_url: `${base}/success?session_id={CHECKOUT_SESSION_ID}`,
    cancel_url: `${base}/cancel`,
    allow_promotion_codes: 'true',
  });
  return Response.redirect(session.url, 303);
}

export async function success(env, url, now) {
  const id = url.searchParams.get('session_id') ?? '';
  if (!/^cs_[A-Za-z0-9_]+$/.test(id)) return page('Genix', '<h1>Неверная ссылка</h1>');
  const session = await stripe(env, 'GET', `/checkout/sessions/${id}?expand[]=subscription`);
  const sub = session.subscription;
  if (session.status !== 'complete' || !sub || !PAID.has(sub.status)) {
    return page('Genix', '<h1>Оплата ещё не прошла</h1><p>Если деньги списаны, обновите страницу через минуту.</p>');
  }
  const email = session.customer_details?.email ?? session.customer_email ?? '';
  const token = await keyForSubscription(env, sub, email, session.customer, now);
  return page('Genix Pro', `<h1>Спасибо! Genix Pro активна</h1><p>Скопируйте ключ и вставьте его в Genix: Справка → Подписка → «Активировать».</p>
<textarea id="k" readonly>${esc(token)}</textarea><p><button onclick="navigator.clipboard.writeText(document.getElementById('k').value)">Скопировать ключ</button></p>
<p>Ключ обновляется сам, пока подписка идёт. Отменить её можно в любой момент из окна подписки в программе.</p>`);
}

export async function refresh(env, request, now) {
  let body;
  try { body = await request.json(); } catch { return json({ error: 'Нужен JSON с полем key' }, 400); }
  const claims = await readKey(env, body?.key);
  if (!claims) return json({ error: 'Ключ не распознан' }, 400);
  if (!claims.cid) return json({ error: 'Это старый ключ без привязки к оплате: купите подписку' }, 400);
  const subs = await stripe(env, 'GET', `/subscriptions?customer=${encodeURIComponent(claims.cid)}&status=all&limit=10`);
  const live = subs.data.filter((s) => PAID.has(s.status)).sort((a, b) => periodEnd(b) - periodEnd(a))[0];
  if (!live) return json({ error: 'Подписка не оплачена или отменена' }, 402);
  return json({ key: await keyForSubscription(env, live, claims.email, claims.cid, now) });
}

export async function manage(env, url) {
  const claims = await readKey(env, url.searchParams.get('key'));
  if (!claims?.cid) return page('Genix', '<h1>Ключ не распознан</h1>');
  const portal = await stripe(env, 'POST', '/billing_portal/sessions', { customer: claims.cid, return_url: env.PUBLIC_BASE_URL || url.origin });
  return Response.redirect(portal.url, 303);
}

/** Stripe-Signature check: HMAC-SHA256 over `${t}.${body}`, five-minute tolerance. */
export async function verifyWebhook(secret, header, body, now) {
  const parts = Object.fromEntries((header ?? '').split(',').map((p) => p.split('=')));
  if (!parts.t || !parts.v1 || Math.abs(now - Number(parts.t)) > 300) return false;
  const key = await crypto.subtle.importKey('raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  const mac = new Uint8Array(await crypto.subtle.sign('HMAC', key, enc.encode(`${parts.t}.${body}`)));
  const hex = [...mac].map((b) => b.toString(16).padStart(2, '0')).join('');
  return hex.length === parts.v1.length && hex.split('').every((c, i) => c === parts.v1[i]);
}

export async function webhook(env, request, now) {
  const body = await request.text();
  if (!(await verifyWebhook(env.STRIPE_WEBHOOK_SECRET, request.headers.get('Stripe-Signature'), body, now))) return json({ error: 'bad signature' }, 400);
  // Keys are issued and renewed on demand from Stripe's current state, so nothing has to be stored here. The hook exists so
  // Stripe sees a healthy endpoint and so events can be logged or mailed later.
  const event = JSON.parse(body);
  console.log('stripe event', event.type, event.id);
  return json({ received: true });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const now = Math.floor(Date.now() / 1000);
    try {
      if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers: { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Headers': 'Content-Type' } });
      if (request.method === 'GET' && url.pathname === '/buy') return await buy(env, url);
      if (request.method === 'GET' && url.pathname === '/success') return await success(env, url, now);
      if (request.method === 'GET' && url.pathname === '/cancel') return page('Genix', '<h1>Оплата отменена</h1><p>Ничего не списано. Можно вернуться в Genix.</p>');
      if (request.method === 'GET' && url.pathname === '/manage') return await manage(env, url);
      if (request.method === 'POST' && url.pathname === '/refresh') return await refresh(env, request, now);
      if (request.method === 'POST' && url.pathname === '/webhook') return await webhook(env, request, now);
      if (url.pathname === '/health') return json({ ok: true });
      return json({ error: 'not found' }, 404);
    } catch (e) {
      return json({ error: String(e.message ?? e) }, 502);
    }
  },
};
