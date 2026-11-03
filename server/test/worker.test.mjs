import test from 'node:test';
import assert from 'node:assert/strict';
import worker, { b64u, readKey, signKey, verifyWebhook, periodEnd } from '../src/worker.js';

const DAY = 86400;
const NOW = 1_800_000_000;

async function makeEnv() {
  const pair = await crypto.subtle.generateKey('Ed25519', true, ['sign', 'verify']);
  const pkcs8 = Buffer.from(await crypto.subtle.exportKey('pkcs8', pair.privateKey)).toString('base64');
  const pub = Buffer.from(await crypto.subtle.exportKey('raw', pair.publicKey)).toString('hex');
  return { LICENSE_PRIVATE_KEY: pkcs8, LICENSE_PUBLIC_KEY: pub, STRIPE_SECRET_KEY: 'sk_test_x', STRIPE_WEBHOOK_SECRET: 'whsec_x', STRIPE_PRICE_ID: 'price_1', PUBLIC_BASE_URL: 'https://api.test', GRACE_DAYS: '3' };
}

/** Replaces fetch with a fake Stripe; returns the list of requests it saw. */
function fakeStripe(routes) {
  const seen = [];
  globalThis.fetch = async (url, init = {}) => {
    const path = new URL(url).pathname.replace('/v1', '') + new URL(url).search;
    seen.push({ method: init.method, path, body: init.body });
    for (const [pattern, reply] of Object.entries(routes)) {
      if (path.startsWith(pattern)) return new Response(JSON.stringify(reply), { status: reply.error ? 400 : 200 });
    }
    return new Response(JSON.stringify({ error: { message: 'unexpected ' + path } }), { status: 404 });
  };
  return seen;
}

const call = (env, method, path, body, headers = {}) => worker.fetch(new Request('https://api.test' + path, { method, body, headers }), env);

test('a signed key reads back, and a forged or damaged one does not', async () => {
  const env = await makeEnv();
  const token = await signKey(env, { email: 'a@b.c', cid: 'cus_1', until: NOW + DAY });
  assert.deepEqual(await readKey(env, token), { email: 'a@b.c', cid: 'cus_1', until: NOW + DAY });
  const other = await makeEnv();
  assert.equal(await readKey(other, token), null, 'another vendor key');
  const [p, s] = token.split('.');
  const changed = b64u.encode(new TextEncoder().encode(JSON.stringify({ email: 'a@b.c', cid: 'cus_1', until: NOW + 999 * DAY })));
  assert.equal(await readKey(env, `${changed}.${s}`), null, 'changed payload');
  for (const junk of ['', 'abc', '.', 'a.b', undefined]) assert.equal(await readKey(env, junk), null);
});

test('/buy creates a subscription checkout and redirects to it', async () => {
  const env = await makeEnv();
  const seen = fakeStripe({ '/checkout/sessions': { url: 'https://checkout.stripe.com/c/pay/cs_1' } });
  const res = await call(env, 'GET', '/buy');
  assert.equal(res.status, 303);
  assert.equal(res.headers.get('Location'), 'https://checkout.stripe.com/c/pay/cs_1');
  const form = new URLSearchParams(seen[0].body);
  assert.equal(form.get('mode'), 'subscription');
  assert.equal(form.get('line_items[0][price]'), 'price_1');
  assert.equal(form.get('success_url'), 'https://api.test/success?session_id={CHECKOUT_SESSION_ID}');
});

test('/success shows a key only for a completed, paid session', async () => {
  const env = await makeEnv();
  const end = Math.floor(Date.now() / 1000) + 30 * DAY;
  fakeStripe({ '/checkout/sessions/cs_ok': { status: 'complete', customer: 'cus_9', customer_details: { email: 'me@x.y' }, subscription: { status: 'active', current_period_end: end } } });
  const res = await call(env, 'GET', '/success?session_id=cs_ok');
  const text = await res.text();
  const token = text.match(/<textarea[^>]*>([^<]+)</)[1];
  const claims = await readKey(env, token);
  assert.equal(claims.email, 'me@x.y');
  assert.equal(claims.cid, 'cus_9');
  assert.equal(claims.until, end + 3 * DAY, 'period end plus the grace days');

  fakeStripe({ '/checkout/sessions/cs_open': { status: 'open', subscription: null } });
  assert.match(await (await call(env, 'GET', '/success?session_id=cs_open')).text(), /ещё не прошла/);
  assert.match(await (await call(env, 'GET', '/success?session_id=../evil')).text(), /Неверная ссылка/);
});

test('the new Stripe layout (period end on the subscription item) is understood', () => {
  assert.equal(periodEnd({ items: { data: [{ current_period_end: 77 }] } }), 77);
  assert.equal(periodEnd({ current_period_end: 5, items: { data: [{ current_period_end: 77 }] } }), 5);
  assert.equal(periodEnd({}), 0);
});

test('/refresh renews while the subscription lives and refuses once it is cancelled', async () => {
  const env = await makeEnv();
  const old = await signKey(env, { email: 'me@x.y', cid: 'cus_9', until: 1 });
  const end = Math.floor(Date.now() / 1000) + 25 * DAY;
  fakeStripe({ '/subscriptions': { data: [{ status: 'canceled', current_period_end: end + 90 * DAY }, { status: 'active', current_period_end: end }] } });
  let res = await call(env, 'POST', '/refresh', JSON.stringify({ key: old }));
  assert.equal(res.status, 200);
  const fresh = await readKey(env, (await res.json()).key);
  assert.equal(fresh.until, end + 3 * DAY, 'a cancelled subscription never sets the date');
  assert.equal(fresh.cid, 'cus_9');

  fakeStripe({ '/subscriptions': { data: [{ status: 'canceled', current_period_end: end }, { status: 'past_due', current_period_end: end }] } });
  res = await call(env, 'POST', '/refresh', JSON.stringify({ key: old }));
  assert.equal(res.status, 402);
  assert.ok((await res.json()).error);

  assert.equal((await call(env, 'POST', '/refresh', 'not json')).status, 400);
  assert.equal((await call(env, 'POST', '/refresh', JSON.stringify({ key: 'junk' }))).status, 400);
  const noCid = await signKey(env, { email: 'a@b.c', until: 9 });
  assert.equal((await call(env, 'POST', '/refresh', JSON.stringify({ key: noCid }))).status, 400);
});

test('/manage sends a valid key holder to the Stripe portal and nobody else', async () => {
  const env = await makeEnv();
  const token = await signKey(env, { email: 'me@x.y', cid: 'cus_9', until: 1 });
  const seen = fakeStripe({ '/billing_portal/sessions': { url: 'https://billing.stripe.com/p/s1' } });
  const res = await call(env, 'GET', '/manage?key=' + encodeURIComponent(token));
  assert.equal(res.status, 303);
  assert.equal(new URLSearchParams(seen[0].body).get('customer'), 'cus_9');
  assert.match(await (await call(env, 'GET', '/manage?key=junk')).text(), /не распознан/);
});

async function stripeSig(secret, body, t) {
  const key = await crypto.subtle.importKey('raw', new TextEncoder().encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  const mac = new Uint8Array(await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(`${t}.${body}`)));
  return `t=${t},v1=${[...mac].map((b) => b.toString(16).padStart(2, '0')).join('')}`;
}

test('webhooks need a correct, fresh Stripe signature', async () => {
  const body = JSON.stringify({ id: 'evt_1', type: 'invoice.paid' });
  const good = await stripeSig('whsec_x', body, NOW);
  assert.equal(await verifyWebhook('whsec_x', good, body, NOW + 10), true);
  assert.equal(await verifyWebhook('whsec_x', good, body + ' ', NOW + 10), false, 'changed body');
  assert.equal(await verifyWebhook('whsec_other', good, body, NOW + 10), false, 'wrong secret');
  assert.equal(await verifyWebhook('whsec_x', good, body, NOW + 3600), false, 'replayed an hour later');
  assert.equal(await verifyWebhook('whsec_x', undefined, body, NOW), false);
  assert.equal(await verifyWebhook('whsec_x', 't=1', body, NOW), false);

  const env = await makeEnv();
  const t = Math.floor(Date.now() / 1000);
  const ok = await call(env, 'POST', '/webhook', body, { 'Stripe-Signature': await stripeSig('whsec_x', body, t) });
  assert.equal(ok.status, 200);
  const bad = await call(env, 'POST', '/webhook', body, { 'Stripe-Signature': 'v1=00,t=1' });
  assert.equal(bad.status, 400);
});

test('unknown paths and Stripe failures answer with an error, not a crash', async () => {
  const env = await makeEnv();
  assert.equal((await call(env, 'GET', '/nope')).status, 404);
  fakeStripe({});
  const res = await call(env, 'GET', '/buy');
  assert.equal(res.status, 502);
  assert.match((await res.json()).error, /unexpected/);
});

test('a key made by this server is the format the app expects (fixture for the Rust test)', async () => {
  const env = await makeEnv();
  const token = await signKey(env, { email: 'fix@x.y', cid: 'cus_f', until: 4_000_000_000 });
  assert.match(token, /^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]{86}$/, 'unpadded base64url, 64-byte signature');
  if (process.env.PRINT_FIXTURE) console.log(JSON.stringify({ token, pub: env.LICENSE_PUBLIC_KEY }));
});
