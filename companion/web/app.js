'use strict';
// X3 Companion page. Same-origin API only; Cloudflare Access handles login.
const $ = (id) => document.getElementById(id);
const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children.filter((c) => c != null && c !== ''));
  return node;
};

const STATES = {
  accepted: 'Waiting',
  preparing: 'Preparing',
  ready: 'Ready for Receive',
  sending: 'Sending',
  delivered: 'On the reader',
  displayed: 'On screen',
  failed: 'Failed',
  cancelled: 'Cancelled',
};
const KINDS = { article: 'Article', token: 'Token', image: 'Picture', delete: 'Removal' };

function say(message, tone = 'ok') {
  const status = $('status');
  status.dataset.tone = tone;
  status.textContent = message;
  if (message) status.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
}

function explain(status, context) {
  switch (status) {
    case 400: return context === 'link'
      ? 'That link can’t be used. Use a public web address that starts with https:// or http://, without a port number.'
      : 'Something in the form isn’t valid. Check it and try again.';
    case 401: return 'Your login has expired. Reload the page to sign in again.';
    case 404: return context === 'saved' ? 'That saved item is gone. The list has been refreshed.' : 'That item no longer exists. Refresh the inbox.';
    case 409: return context === 'picture' ? 'The picture changed since the preview. Choose the settings again.' : 'This was already sent, or the card image changed. Choose the card again, or use “Remove from reader” instead.';
    case 413: return context === 'picture' ? 'That image is too big. Pictures can be up to 8 MB.' : 'That’s too long. An article can be up to 1 MB of text.';
    case 415: if (context === 'picture') return 'Use a JPEG, PNG, WebP or GIF image.'; return 'The page sent the wrong content type. Reload and try again.';
    case 422: return context === 'picture' ? 'Couldn’t read this image, or it is more than 4096 pixels on a side.' : 'Couldn’t turn this into something the X3 can show.';
    case 502: return 'Scryfall didn’t answer. Try again in a minute.';
    case 503: return 'The companion is busy. Try again in a moment.';
    case 507: return 'The inbox is full. Delivered items are cleared after 30 days.';
    default: return `Something went wrong (${status}). Try again.`;
  }
}

async function api(path, options = {}, context = '') {
  let response;
  try {
    response = await fetch(path, {
      ...options,
      headers: { 'Content-Type': 'application/json', ...options.headers },
      credentials: 'same-origin',
    });
  } catch {
    throw Object.assign(new Error('Can’t reach the companion. Check your connection and try again.'), { status: 0 });
  }
  if (!response.ok) {
    throw Object.assign(new Error(explain(response.status, context)), { status: response.status });
  }
  return response;
}

// Run an action from a form or button: shows errors and blocks double taps.
function action(fn, button) {
  return async (event) => {
    event?.preventDefault();
    const target = button ?? event?.submitter;
    if (target?.getAttribute('aria-busy') === 'true') return;
    target?.setAttribute('aria-busy', 'true');
    try {
      await fn(event);
    } catch (error) {
      say(error.message, 'error');
    } finally {
      target?.removeAttribute('aria-busy');
    }
  };
}

// One submission ID per unchanged body, so a retry after a dropped
// connection is recognised by the companion instead of queued twice.
let pending = null;
function submissionId(key) {
  if (!pending || pending.key !== key) pending = { key, id: crypto.randomUUID() };
  return pending.id;
}
async function submit(path, body, context) {
  const id = submissionId(JSON.stringify([path, body]));
  const response = await api(path, { method: 'POST', body: JSON.stringify({ ...body, submission_id: id }) }, context);
  pending = null;
  await refresh();
  return response.json();
}

// Small per-viewer conveniences; the page works without them.
const remember = {
  get(key) { try { return localStorage.getItem(key); } catch { return null; } },
  set(key, value) { try { localStorage.setItem(key, value); } catch { /* private mode */ } },
};

/* Mode switch: the last one used comes back next time. */
function showMode() {
  const mode = document.querySelector('input[name="mode"]:checked').value;
  for (const panel of document.querySelectorAll('.panel')) panel.hidden = panel.dataset.mode !== mode;
  remember.set('x3.mode', mode);
  say('');
}
for (const radio of document.querySelectorAll('input[name="mode"]')) radio.addEventListener('change', showMode);
const lastMode = document.querySelector(`input[name="mode"][value="${remember.get('x3.mode')}"]`);
if (lastMode) { lastMode.checked = true; showMode(); }

const picked = (name) => document.querySelector(`input[name="${name}"]:checked`)?.value;

/* Articles */
$('link-form').addEventListener('submit', action(async () => {
  const url = $('link-url').value.trim();
  await submit('/api/v1/articles/link', { kind: picked('link-kind'), url }, 'link');
  $('link-url').value = '';
  say('Queued. The companion is reading the page now.');
}));

$('paste-form').addEventListener('submit', action(async () => {
  await submit('/api/v1/articles', {
    kind: picked('paste-kind'),
    title: $('paste-title').value.trim(),
    source_url: $('paste-url').value.trim(),
    text: $('paste-text').value,
  }, 'paste');
  $('paste-form').reset();
  say('Queued. It’s ready for the next Receive.');
}));

/* Screen previews: shared by tokens and pictures. A newer request wins. */
function previewer({ figure, caption, queue, ready }) {
  let revision = 0;
  let url = null;
  let hash = null;
  const img = el('img', { decoding: 'async' });
  return {
    get hash() { return hash; },
    clear() {
      revision++;
      hash = null;
      queue.disabled = true;
      figure.hidden = true;
    },
    async load(request, alt) {
      const rev = ++revision;
      hash = null;
      queue.disabled = true;
      figure.hidden = false;
      figure.setAttribute('aria-busy', 'true');
      caption.textContent = 'Preparing the preview…';
      try {
        const response = await request();
        const blob = await response.blob();
        if (rev !== revision) return;
        const sha = response.headers.get('X3-Payload-Sha256');
        if (!sha) throw new Error('The preview came back incomplete. Try again.');
        if (url) URL.revokeObjectURL(url);
        url = URL.createObjectURL(blob);
        img.src = url;
        img.alt = alt;
        if (!img.isConnected) figure.prepend(img);
        caption.textContent = 'These are the exact pixels the X3 will show.';
        hash = sha;
        queue.disabled = false;
        ready?.();
      } catch (error) {
        if (rev !== revision) return;
        figure.hidden = true;
        throw error;
      } finally {
        if (rev === revision) figure.removeAttribute('aria-busy');
      }
    },
  };
}

/* Tokens */
const COLOURS = { W: 'white', U: 'blue', B: 'black', R: 'red', G: 'green' };
function colours(list) {
  if (!Array.isArray(list)) return '';
  if (list.length === 0) return 'colourless';
  return list.map((c) => COLOURS[c] ?? c).join(' and ');
}
// "1/1 white Soldier creature", from the face's type line.
function describe(face) {
  const kinds = (face.type ?? '').replace(/^Token\s+/, '').split(' — ');
  const types = (kinds[0] ?? '').toLowerCase();
  const sub = kinds[1] ?? '';
  const line = [face.pt, colours(face.colors), sub, types].filter(Boolean).join(' ');
  return line.charAt(0).toUpperCase() + line.slice(1);
}

let cards = new Map();
let shownResults = 0;
const PAGE = 8;
const tokenPreview = previewer({ figure: $('screen'), caption: $('screen-caption'), queue: $('queue') });

function spec() {
  return {
    scryfall_id: picked('card'),
    face: Number(picked('face') ?? 0),
    fit: 'contain',
    orientation: picked('orientation'),
  };
}

function resultRow(card) {
  const faces = card.faces ?? [];
  const front = faces[0] ?? {};
  const input = el('input', { type: 'radio', name: 'card', value: card.id });
  const thumbs = el('span', { className: 'thumbs' }, ...faces.map((f) => el('img', {
    src: f.thumb ?? '', alt: '', loading: 'lazy', decoding: 'async', width: 146, height: 204,
  })));
  const text = (front.text ?? '').split('\n')[0];
  const body = el('span', { className: 'result-text' },
    el('span', { className: 'result-name', textContent: card.name }),
    el('span', { className: 'result-kind', textContent: faces.length > 1 ? `Two-sided · ${faces.map(describe).map((d) => d.charAt(0).toLowerCase() + d.slice(1)).join(' / ')}` : describe(front) }),
    text ? el('span', { className: 'result-rules', textContent: text }) : null,
    el('span', { className: 'result-set', textContent: [card.set, card.year].filter(Boolean).join(' · ') }));
  return el('label', { className: 'result' }, input, thumbs, body);
}

function showResults(count) {
  const list = $('result-list');
  const all = [...cards.values()];
  for (const card of all.slice(shownResults, count)) list.append(resultRow(card));
  shownResults = Math.min(count, all.length);
  $('more-results').hidden = shownResults >= all.length;
  $('more-results').textContent = `Show ${Math.min(PAGE, all.length - shownResults)} more`;
}

$('search-form').addEventListener('submit', action(async () => {
  tokenPreview.clear();
  const words = $('query').value.trim();
  let data;
  try {
    data = await (await api('/api/v1/scryfall/search?q=' + encodeURIComponent(words))).json();
  } catch (error) {
    // Scryfall answers 404 when nothing matches; the proxy reports it as 502.
    if (error.status === 502) data = { data: [], total: 0 };
    else throw error;
  }
  $('result-list').replaceChildren();
  cards = new Map((data.data ?? []).map((card) => [card.id, card]));
  shownResults = 0;
  showResults(PAGE);
  $('results').hidden = cards.size === 0;
  $('token-form').hidden = true;
  const total = data.total ?? cards.size;
  if (!cards.size) say(`No token matches “${words}”. Try a shorter name, such as “Soldier”.`, 'error');
  else if (total > cards.size) say(`The ${cards.size} closest of ${total} tokens. Add a word to narrow it down.`);
  else say(`${total} ${total === 1 ? 'token' : 'tokens'} found. Choose one.`);
}));
$('more-results').addEventListener('click', () => showResults(shownResults + PAGE));

function previewToken() {
  const s = spec();
  if (!s.scryfall_id) return;
  const card = cards.get(s.scryfall_id);
  const face = card?.faces?.[s.face]?.name ?? card?.name ?? 'Token';
  return tokenPreview.load(
    () => api('/api/v1/tokens/preview.png?' + new URLSearchParams(s)),
    `${face} as the X3 will show it`,
  ).catch((error) => say(error.message, 'error'));
}

$('result-list').addEventListener('change', () => {
  const card = cards.get(picked('card'));
  const faces = card?.faces ?? [];
  const field = $('face-field');
  field.replaceChildren(el('legend', { textContent: 'Side' }));
  if (faces.length > 1) {
    faces.forEach((f, i) => field.append(el('label', { className: 'face' },
      el('input', { type: 'radio', name: 'face', value: String(i), checked: i === 0 }),
      el('img', { src: f.thumb ?? '', alt: '', decoding: 'async', width: 146, height: 204 }),
      el('span', { textContent: f.name ?? (i ? 'Back' : 'Front') }))));
  }
  field.hidden = faces.length < 2;
  $('token-form').hidden = false;
  say('');
  previewToken();
});
$('token-form').addEventListener('change', (event) => {
  if (['face', 'orientation'].includes(event.target.name)) previewToken();
});

$('token-form').addEventListener('submit', action(async () => {
  const hash = tokenPreview.hash;
  if (!hash) return;
  const s = spec();
  const card = cards.get(s.scryfall_id);
  const name = card?.faces?.length > 1 ? card.faces[s.face]?.name : card?.name;
  await submit('/api/v1/tokens', { ...s, expected_sha256: hash, show: $('show').checked, ...(name ? { name } : {}) }, 'token');
  say($('show').checked ? 'Queued. It will be on screen after the next Receive.' : 'Queued. It will be saved on the X3 after the next Receive.');
}));

/* Pictures (ADR 028): shrink on this device, preview, then queue the same bytes. */
let pictureBlob = null;
const picturePreview = previewer({ figure: $('picture-screen'), caption: $('picture-caption'), queue: $('picture-queue') });

// Longest side at most 2048 px, sent as JPEG (PNG keeps transparency).
async function shrink(file) {
  const bitmap = await createImageBitmap(file).catch(() => {
    throw new Error('This file is not an image the browser can read.');
  });
  const scale = Math.min(1, 2048 / Math.max(bitmap.width, bitmap.height));
  if (scale === 1 && file.size <= 8 * 1024 * 1024 && ['image/jpeg', 'image/png', 'image/webp', 'image/gif'].includes(file.type)) {
    bitmap.close();
    return file;
  }
  const canvas = Object.assign(document.createElement('canvas'), {
    width: Math.round(bitmap.width * scale),
    height: Math.round(bitmap.height * scale),
  });
  canvas.getContext('2d').drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  const type = file.type === 'image/png' ? 'image/png' : 'image/jpeg';
  return new Promise((resolve, reject) => canvas.toBlob(
    (b) => (b ? resolve(b) : reject(new Error('Could not prepare this image. Try another file.'))),
    type,
    0.9,
  ));
}

const pictureSettings = () => ({ fit: picked('picture-fit'), orientation: picked('picture-orientation') });

function previewPicture() {
  if (!pictureBlob) return;
  const s = pictureSettings();
  return picturePreview.load(
    () => api('/api/v1/images/preview.png?' + new URLSearchParams(s), {
      method: 'POST', body: pictureBlob, headers: { 'Content-Type': pictureBlob.type },
    }, 'picture'),
    `${$('picture-name').value || 'Your picture'} as the X3 will show it`,
  ).catch((error) => say(error.message, 'error'));
}

$('picture-file').addEventListener('change', action(async () => {
  picturePreview.clear();
  pictureBlob = null;
  const file = $('picture-file').files[0];
  if (!file) return;
  pictureBlob = await shrink(file);
  if (!$('picture-name').value) $('picture-name').value = file.name.replace(/\.[^.]+$/, '').replace(/[_-]+/g, ' ').slice(0, 200);
  say('');
  await previewPicture();
}));
$('picture-form').addEventListener('change', (event) => {
  if (['picture-fit', 'picture-orientation'].includes(event.target.name)) previewPicture();
});

$('picture-form').addEventListener('submit', action(async () => {
  const hash = picturePreview.hash;
  if (!hash || !pictureBlob) return;
  const name = $('picture-name').value.trim();
  const params = { ...pictureSettings(), expected_sha256: hash, show: $('picture-show').checked ? '1' : '0', ...(name ? { name } : {}) };
  const id = submissionId(JSON.stringify(['picture', params, pictureBlob.size]));
  await api('/api/v1/images?' + new URLSearchParams({ ...params, submission_id: id }), {
    method: 'POST', body: pictureBlob, headers: { 'Content-Type': pictureBlob.type },
  }, 'picture');
  pending = null;
  await refresh();
  say($('picture-show').checked ? 'Queued. It will be on screen after the next Receive.' : 'Queued. It will be saved on the X3 after the next Receive.');
}));

/* Saved shelf: tokens and pictures the X3 already has once; send again at once. */
const SHELF = 8;
const shelves = [...document.querySelectorAll('.saved')].map((section) => ({
  section,
  kind: section.dataset.kind,
  open: false,
  choice: null,
}));

function shelfButton(text, className, onClick) {
  const b = el('button', { type: 'button', className, textContent: text });
  b.addEventListener('click', action(onClick, b));
  return b;
}

function renderShelf(shelf, entries) {
  const { section, kind } = shelf;
  const mine = entries.filter((e) => e.kind === kind);
  section.hidden = mine.length === 0;
  const heading = section.querySelector('h3');
  section.replaceChildren(heading);
  if (!mine.length) return;
  if (!mine.some((e) => e.id === shelf.choice)) shelf.choice = null;
  const label = kind === 'token' ? 'token' : 'picture';
  const grid = el('fieldset', { className: 'saved-grid' }, el('legend', { className: 'visually-hidden', textContent: `Choose a saved ${label}` }));
  for (const entry of shelf.open ? mine : mine.slice(0, SHELF)) {
    const name = entry.title || (kind === 'token' ? 'Token' : 'Picture');
    grid.append(el('label', { className: 'saved-item' },
      el('input', { type: 'radio', name: `saved-${kind}`, value: entry.id, checked: entry.id === shelf.choice }),
      el('img', { src: `/api/v1/saved/${entry.id}/preview.png`, alt: '', loading: 'lazy', decoding: 'async' }),
      el('span', { textContent: name })));
  }
  grid.addEventListener('change', () => {
    shelf.choice = grid.querySelector('input:checked')?.value ?? null;
    renderShelf(shelf, entries);
  });
  section.append(el('p', { className: 'hint', textContent: `Sent before. Send one again without a new ${kind === 'token' ? 'search' : 'upload'}.` }), grid);
  if (mine.length > SHELF) {
    section.append(shelfButton(shelf.open ? 'Show fewer' : `Show all ${mine.length}`, 'quiet small', () => {
      shelf.open = !shelf.open;
      renderShelf(shelf, entries);
    }));
  }
  const chosen = mine.find((e) => e.id === shelf.choice);
  if (!chosen) return;
  const show = el('input', { type: 'checkbox', checked: true });
  section.append(el('div', { className: 'saved-actions' },
    el('label', { className: 'check' }, show, 'Show it on the X3 after Receive'),
    el('div', { className: 'button-row' },
      shelfButton('Send again', 'primary', async () => {
        await submit(`/api/v1/saved/${chosen.id}/send`, { show: show.checked }, 'saved');
        say(`${chosen.title || 'It'} is queued again. ${show.checked ? 'It will be on screen after the next Receive.' : 'It will be on the X3 after the next Receive.'}`);
        shelf.choice = null;
        await loadSaved();
      }),
      shelfButton('Forget', 'quiet', async () => {
        await api(`/api/v1/saved/${chosen.id}`, { method: 'DELETE' }, 'saved');
        say(`${chosen.title || 'It'} is no longer saved here. The X3 keeps its copy.`);
        shelf.choice = null;
        await loadSaved();
      }))));
}

async function loadSaved() {
  let entries = [];
  try {
    entries = (await (await api('/api/v1/saved')).json()).saved ?? [];
  } catch (error) {
    if (error.status !== 404) throw error;
  }
  for (const shelf of shelves) renderShelf(shelf, entries);
}

/* Inbox: the newest ten, then older ones on request. */
const ago = new Intl.RelativeTimeFormat('en-GB', { numeric: 'auto' });
function when(seconds) {
  const diff = seconds - Date.now() / 1000;
  for (const [unit, size] of [['day', 86400], ['hour', 3600], ['minute', 60]]) {
    if (Math.abs(diff) >= size) return ago.format(Math.round(diff / size), unit);
  }
  return 'just now';
}

function button(text, onClick) {
  const b = el('button', { type: 'button', className: 'quiet small', textContent: text });
  b.addEventListener('click', action(onClick, b));
  return b;
}

// Cancelling cannot be undone, so it takes a second tap within 4 seconds.
function confirmButton(text, confirmText, onConfirm) {
  const b = el('button', { type: 'button', className: 'quiet small', textContent: text });
  let armed = null;
  b.addEventListener('click', action(async () => {
    if (!armed) {
      b.textContent = confirmText;
      b.dataset.armed = 'true';
      armed = setTimeout(() => { armed = null; b.textContent = text; delete b.dataset.armed; }, 4000);
      return;
    }
    clearTimeout(armed);
    armed = null;
    await onConfirm();
  }, b));
  return b;
}

function title(job) {
  if (job.title) return job.kind === 'delete' ? `Remove “${job.title}”` : job.title;
  return { article: 'Untitled article', token: 'Token', image: 'Picture', delete: 'Remove an item' }[job.kind] ?? job.item;
}

function jobRow(job) {
  const li = el('li', { className: 'job' });
  const heading = el('p', { className: 'job-title', textContent: title(job) });
  if (/^https?:\/\//.test(job.title ?? '')) heading.classList.add('link');
  const state = el('span', { className: 'state', textContent: STATES[job.state] ?? job.state });
  state.dataset.state = job.state;
  const time = el('time', { dateTime: new Date(job.created * 1000).toISOString(), textContent: when(job.created) });
  li.append(heading, el('p', { className: 'job-meta' }, state, el('span', { textContent: KINDS[job.kind] ?? job.kind }), time));

  if (job.state === 'failed') {
    li.append(el('p', {
      className: 'job-note',
      textContent: job.kind === 'article'
        ? 'Couldn’t prepare this. If it’s a link, check the page opens without a login, or paste the text instead.'
        : 'Couldn’t prepare this. Try queueing it again.',
    }));
  }

  const actions = el('div', { className: 'job-actions' });
  if (job.kind !== 'delete' && job.len > 0) {
    actions.append(button(job.kind === 'article' ? 'Read it as the X3 will' : 'See it on the X3', () => openViewer(job)));
  }
  if (job.state === 'failed' && /^https?:\/\//.test(job.title ?? '')) {
    actions.append(button('Try again', async () => {
      await submit('/api/v1/articles/link', { kind: 'article', url: job.title }, 'link');
      say('Queued again. The companion is reading the page now.');
    }));
  }
  if (!job.delivery_started && ['accepted', 'preparing', 'ready'].includes(job.state)) {
    actions.append(confirmButton('Cancel', 'Tap again to cancel', async () => {
      await api(`/api/v1/jobs/${encodeURIComponent(job.job)}/cancel`, { method: 'POST' });
      say('Cancelled. It won’t go to the X3.');
      await refresh();
    }));
  }
  if (job.kind !== 'delete' && ['delivered', 'displayed'].includes(job.state)) {
    actions.append(button('Remove from reader', async () => {
      await api('/api/v1/items/' + encodeURIComponent(job.item), { method: 'DELETE' });
      say('It will be removed at the next Receive.');
      await refresh();
    }));
  }
  if (actions.childElementCount) li.append(actions);
  return li;
}

const inbox = { newest: [], older: [], more: false };
function render() {
  const seen = new Set();
  const jobs = [...inbox.newest, ...inbox.older].filter((j) => !seen.has(j.job) && seen.add(j.job));
  $('jobs').replaceChildren(...jobs.map(jobRow));
  $('empty').hidden = jobs.length > 0;
  $('older').hidden = !inbox.more;
}

$('older').addEventListener('click', action(async () => {
  const last = inbox.older.at(-1) ?? inbox.newest.at(-1);
  if (!last) return;
  const page = await (await api(`/api/v1/jobs?limit=10&before=${encodeURIComponent(last.job)}`)).json();
  inbox.older.push(...(page.jobs ?? []));
  inbox.more = Boolean(page.more);
  render();
}, $('older')));

/* Screen viewer: pages through a queued item exactly as the X3 will show it. */
const viewer = { job: null, page: 0, pages: 1, url: null, rev: 0 };
async function showPage(page) {
  const rev = ++viewer.rev;
  const response = await api(`/api/v1/jobs/${encodeURIComponent(viewer.job.job)}/preview.png?page=${page}`);
  const blob = await response.blob();
  if (rev !== viewer.rev) return;
  viewer.page = page;
  viewer.pages = Number(response.headers.get('X3-Page-Count')) || 1;
  if (viewer.url) URL.revokeObjectURL(viewer.url);
  viewer.url = URL.createObjectURL(blob);
  const img = $('viewer-page') ?? el('img', { id: 'viewer-page' });
  img.src = viewer.url;
  img.alt = `Screen ${page + 1} of ${viewer.pages}: ${title(viewer.job)}`;
  if (!img.isConnected) document.querySelector('#viewer .screen').prepend(img);
  $('viewer-count').textContent = `Page ${page + 1} of ${viewer.pages}`;
  $('viewer-nav').hidden = viewer.pages < 2;
  $('viewer-prev').disabled = page === 0;
  $('viewer-next').disabled = page >= viewer.pages - 1;
}
async function openViewer(job) {
  Object.assign(viewer, { job, page: 0, pages: 1 });
  $('viewer-title').textContent = title(job);
  $('viewer-caption').textContent = job.kind === 'article'
    ? 'Pages break here exactly as they will on the X3.'
    : 'These are the exact pixels the X3 will show.';
  await showPage(0);
  if (!$('viewer').open) $('viewer').showModal();
}
$('viewer-prev').addEventListener('click', action(() => showPage(Math.max(0, viewer.page - 1)), $('viewer-prev')));
$('viewer-next').addEventListener('click', action(() => showPage(Math.min(viewer.pages - 1, viewer.page + 1)), $('viewer-next')));
$('viewer-close').addEventListener('click', () => $('viewer').close());
$('viewer').addEventListener('click', (event) => { if (event.target === $('viewer')) $('viewer').close(); });
$('viewer').addEventListener('keydown', (event) => {
  if (event.key === 'ArrowRight' && viewer.page < viewer.pages - 1) { event.preventDefault(); $('viewer-next').click(); }
  if (event.key === 'ArrowLeft' && viewer.page > 0) { event.preventDefault(); $('viewer-prev').click(); }
});

let timer = null;
let delivered = 0;
async function refresh() {
  clearTimeout(timer);
  const page = await (await api('/api/v1/jobs?limit=10')).json();
  inbox.newest = page.jobs ?? [];
  if (!inbox.older.length) inbox.more = Boolean(page.more);
  render();
  // A token or picture the X3 confirmed joins the saved shelf.
  const done = inbox.newest.filter((j) => ['token', 'image'].includes(j.kind) && ['delivered', 'displayed'].includes(j.state)).length;
  if (done !== delivered) { delivered = done; loadSaved().catch(() => {}); }
  const busy = inbox.newest.some((j) => ['accepted', 'preparing', 'sending'].includes(j.state));
  if (!document.hidden) timer = setTimeout(() => refresh().catch(() => {}), busy ? 4000 : 30000);
}

async function device() {
  const info = (await (await api('/api/v1/device')).json()) ?? {};
  $('device').textContent = info.last_contact
    ? `Your X3 last checked in ${when(info.last_contact)}.`
    : 'Your X3 hasn’t checked in yet.';
}

$('refresh').addEventListener('click', action(async () => { await refresh(); await loadSaved(); }, $('refresh')));
document.addEventListener('visibilitychange', () => {
  if (!document.hidden) refresh().catch(() => {});
});
refresh().catch((error) => say(error.message, 'error'));
loadSaved().catch(() => {});
device().catch(() => { $('device').textContent = 'Couldn’t check the reader.'; });
