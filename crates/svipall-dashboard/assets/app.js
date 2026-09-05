// The panel: connect, list what is waiting, hand each job to the renderer for its modality.
//
// No framework and no bundler. The whole thing is three files the server embeds at build time, so
// there is nothing to install and nothing to keep in step with a lockfile.

(function () {
  'use strict';

  const TOKEN = new URLSearchParams(location.search).get('t') || '';
  const statusEl = document.getElementById('status');
  const jobsEl = document.getElementById('jobs');
  let socket = null;
  // A card being answered is left alone: redrawing it under someone's finger loses their taps.
  const busy = new Set();

  function esc(s) {
    return (s || '').replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }

  // Job rows come from the local queue, but they are still data. A URL that is not http(s) never
  // reaches an attribute.
  function httpUrl(u) { return /^https?:\/\//i.test(u || '') ? u : ''; }

  function assetUrl(id) {
    return '/asset/' + encodeURIComponent(id) + '?t=' + encodeURIComponent(TOKEN);
  }

  function say(text, live) {
    statusEl.textContent = text;
    statusEl.className = live ? 'live' : '';
  }

  function card(job) {
    const node = document.createElement('section');
    node.className = 'card';
    node.dataset.task = job.task_id;
    // The store writes the type as "ImageToText"; the snake-cased spelling this once compared
    // against never matched, so every unclassified image job rendered as a token.
    const modality = job.modality || (/image/i.test(job.job_type || '') ? 'text' : 'token');
    const url = httpUrl(job.page_url);
    node.innerHTML =
      '<h3>' + esc(job.job_type) + ' <span class="badge">' + esc(modality) + '</span></h3>' +
      '<p class="meta">' + (url ? esc(url) : 'no page recorded') + '</p>';

    const ctx = {
      assetUrl: assetUrl,
      submit: function (answer) { submit(job.task_id, answer, node); },
    };
    const render = window.SVIPALL_MODES[modality];
    if (render) {
      node.appendChild(render(job, ctx));
    } else {
      // A modality the panel does not know how to draw is still answerable by a person who can see
      // the page; saying so is better than showing an empty card.
      node.appendChild(Object.assign(document.createElement('p'), {
        className: 'note',
        textContent: 'No control for "' + modality + '" yet — solve it in the browser, or decline.',
      }));
    }

    const decline = document.createElement('button');
    decline.className = 'quiet';
    decline.textContent = "I can't read this";
    decline.onclick = function () { ctx.submit({ kind: 'unknown' }); };
    const note = document.createElement('p');
    note.className = 'note';
    const row = document.createElement('div');
    row.className = 'row';
    row.appendChild(decline);
    node.appendChild(row);
    node.appendChild(note);
    return node;
  }

  function submit(taskId, answer, node) {
    if (!socket || socket.readyState !== 1) return;
    busy.add(taskId);
    socket.send(JSON.stringify({ action: 'solve', taskId: taskId, answer: answer }));
    const note = node.querySelector('.note');
    if (note) note.textContent = 'sending…';
  }

  function draw(jobs) {
    const waiting = (jobs || []).filter(function (j) { return j.status === 'pending' || j.status === 'human'; });
    if (!waiting.length) {
      if (busy.size === 0) jobsEl.innerHTML = document.getElementById('empty').innerHTML;
      return;
    }
    const keep = new Map();
    for (const n of jobsEl.querySelectorAll('.card')) keep.set(n.dataset.task, n);
    jobsEl.innerHTML = '';
    for (const job of waiting) {
      // Redrawing a card someone is answering would lose the taps already on it.
      jobsEl.appendChild(busy.has(job.task_id) && keep.has(job.task_id) ? keep.get(job.task_id) : card(job));
    }
  }

  function reply(msg) {
    busy.delete(msg.taskId);
    const node = jobsEl.querySelector('[data-task="' + msg.taskId + '"]');
    if (!node) return;
    const note = node.querySelector('.note');
    if (!note) return;
    if (msg.type === 'rejected') note.textContent = msg.reason || 'rejected';
    else if (msg.type === 'declined') note.textContent = 'passed to the next attempt';
    else note.textContent = 'sent';
  }

  function connect() {
    if (!TOKEN) {
      say('no token in the URL — open the link the server printed');
      return;
    }
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    socket = new WebSocket(proto + '//' + location.host + '/ws?t=' + encodeURIComponent(TOKEN));
    socket.onopen = function () { say('live', true); };
    socket.onclose = function () {
      say('reconnecting…');
      setTimeout(connect, 2000);
    };
    socket.onmessage = function (e) {
      let msg;
      try { msg = JSON.parse(e.data); } catch (_) { return; }
      if (msg.type === 'pending') draw(msg.jobs);
      else if (msg.taskId) reply(msg);
    };
  }

  connect();
})();
