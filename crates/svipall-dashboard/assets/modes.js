// One renderer per modality, mirroring the registry in Rust.
//
// Adding a modality is adding an entry here and a variant in `svipall_core::answer`. Nothing else in
// the panel knows the names of any of them.
//
// The rule every renderer obeys: coordinates leave here as fractions of the asset, between 0 and 1.
// A phone showing a 1280-wide challenge on a 390-wide screen would otherwise send numbers that look
// entirely plausible and are wrong by a factor of three, and nothing downstream could tell.

(function (global) {
  'use strict';

  function el(tag, props, kids) {
    const n = document.createElement(tag);
    Object.assign(n, props || {});
    for (const k of kids || []) n.appendChild(k);
    return n;
  }

  // Where a pointer landed, as a fraction of the element it landed on.
  function fraction(e, target) {
    const r = target.getBoundingClientRect();
    return {
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    };
  }

  function picture(job, ctx, idx) {
    const a = (job.assets || [])[idx || 0];
    const img = el('img', { alt: 'challenge', draggable: false });
    if (a) img.src = ctx.assetUrl(a.id);
    else if (job.image_data) img.src = 'data:image/png;base64,' + job.image_data;
    return img;
  }

  // A picture that collects taps. `limit` of 0 means as many as the person wants.
  function tapTarget(job, ctx, limit, onChange) {
    const img = picture(job, ctx);
    const wrap = el('div', { className: 'canvas' }, [img]);
    const points = [];
    wrap.addEventListener('pointerdown', function (e) {
      e.preventDefault();
      const p = fraction(e, img);
      if (limit && points.length >= limit) points.length = 0;
      points.push(p);
      for (const old of wrap.querySelectorAll('.dot')) old.remove();
      points.forEach(function (q, i) {
        const dot = el('div', { className: 'dot', textContent: String(i + 1) });
        dot.style.left = (q.x * 100) + '%';
        dot.style.top = (q.y * 100) + '%';
        wrap.appendChild(dot);
      });
      if (onChange) onChange(points);
    });
    return { node: wrap, points: points, clear: function () {
      points.length = 0;
      for (const old of wrap.querySelectorAll('.dot')) old.remove();
    } };
  }

  function textish(kind) {
    return function (job, ctx) {
      const body = el('div', {});
      if ((job.assets || []).length || job.image_data) body.appendChild(
        el('div', { className: 'canvas' }, [picture(job, ctx)])
      );
      const input = el('input', { type: 'text', placeholder: 'what it says', autocomplete: 'off' });
      const go = el('button', { className: 'go', textContent: 'Send' });
      go.onclick = function () { ctx.submit({ kind: kind, value: input.value }); };
      input.onkeydown = function (e) { if (e.key === 'Enter') go.click(); };
      body.appendChild(el('div', { className: 'row' }, [input, go]));
      return body;
    };
  }

  const MODES = {
    token: textish('token'),
    nonce: textish('nonce'),
    text: textish('text'),

    audio: function (job, ctx) {
      const body = el('div', {});
      const a = (job.assets || []).find(function (x) { return x.kind === 'audio'; }) || (job.assets || [])[0];
      if (a) body.appendChild(el('audio', { controls: true, src: ctx.assetUrl(a.id) }));
      const input = el('input', { type: 'text', placeholder: 'what you heard', autocomplete: 'off' });
      const go = el('button', { className: 'go', textContent: 'Send' });
      go.onclick = function () { ctx.submit({ kind: 'text', value: input.value }); };
      input.onkeydown = function (e) { if (e.key === 'Enter') go.click(); };
      body.appendChild(el('div', { className: 'row' }, [input, go]));
      return body;
    },

    tiles: function (job, ctx) {
      const tiles = (job.assets || []).filter(function (a) { return a.kind === 'tile'; });
      const cols = Math.round(Math.sqrt(tiles.length)) || 3;
      const grid = el('div', { className: 'tiles' });
      grid.style.gridTemplateColumns = 'repeat(' + cols + ', 1fr)';
      const chosen = new Set();
      tiles.forEach(function (a, i) {
        const img = el('img', { src: ctx.assetUrl(a.id), alt: 'tile ' + (i + 1), draggable: false });
        img.onclick = function () {
          if (chosen.has(i)) { chosen.delete(i); img.classList.remove('on'); }
          else { chosen.add(i); img.classList.add('on'); }
        };
        grid.appendChild(img);
      });
      const go = el('button', { className: 'go', textContent: 'Send' });
      go.onclick = function () {
        ctx.submit({ kind: 'tiles', indices: Array.from(chosen).sort(function (a, b) { return a - b; }) });
      };
      return el('div', {}, [grid, el('div', { className: 'row' }, [go])]);
    },

    points: function (job, ctx) {
      const t = tapTarget(job, ctx, 0);
      const go = el('button', { className: 'go', textContent: 'Send' });
      go.onclick = function () { ctx.submit({ kind: 'points', points: t.points }); };
      const undo = el('button', { className: 'quiet', textContent: 'Start over' });
      undo.onclick = t.clear;
      return el('div', {}, [t.node, el('div', { className: 'row' }, [go, undo])]);
    },

    polygon: function (job, ctx) {
      // Two ways to trace: tap corners freely, or tap two opposite corners for a rectangle,
      // which is what "draw a box around" asks for and what a detector answers with.
      const rect = el('input', { type: 'checkbox', checked: true });
      const go = el('button', { className: 'go', textContent: 'Send', disabled: true });
      const enough = function (p) { return rect.checked ? p.length === 2 : p.length >= 3; };
      const t = tapTarget(job, ctx, 0, function (p) { go.disabled = !enough(p); });
      rect.onchange = function () { go.disabled = !enough(t.points); };
      go.onclick = function () {
        let pts = t.points.slice();
        if (rect.checked && pts.length === 2) {
          const a = pts[0], b = pts[1];
          pts = [{ x: a.x, y: a.y }, { x: b.x, y: a.y }, { x: b.x, y: b.y }, { x: a.x, y: b.y }];
        }
        ctx.submit({ kind: 'polygon', points: pts });
      };
      const undo = el('button', { className: 'quiet', textContent: 'Start over' });
      undo.onclick = function () { t.clear(); go.disabled = true; };
      return el('div', {}, [
        t.node,
        el('label', { className: 'note' }, [rect, document.createTextNode(' Rectangle: tap two opposite corners. Otherwise tap at least three corners around it.')]),
        el('div', { className: 'row' }, [go, undo]),
      ]);
    },

    drag: function (job, ctx) {
      const go = el('button', { className: 'go', textContent: 'Send', disabled: true });
      const t = tapTarget(job, ctx, 2, function (p) { go.disabled = p.length < 2; });
      go.onclick = function () { ctx.submit({ kind: 'drag', from: t.points[0], to: t.points[1] }); };
      return el('div', {}, [
        t.node,
        el('p', { className: 'note', textContent: 'Tap the piece, then where it belongs.' }),
        el('div', { className: 'row' }, [go]),
      ]);
    },

    slide: function (job, ctx) {
      const img = picture(job, ctx);
      // A thousand steps so the handle can express a pixel on any width of screen.
      const range = el('input', { type: 'range', min: '0', max: '1000', value: '0' });
      const out = el('output', { textContent: '0%' });
      range.oninput = function () { out.textContent = Math.round(range.value / 10) + '%'; };
      const go = el('button', { className: 'go', textContent: 'Send' });
      go.onclick = function () { ctx.submit({ kind: 'slide', fraction: Number(range.value) / 1000 }); };
      return el('div', {}, [
        el('div', { className: 'canvas' }, [img]),
        el('div', { className: 'row' }, [range, out, go]),
      ]);
    },

    rotate: function (job, ctx) {
      const img = picture(job, ctx);
      const range = el('input', { type: 'range', min: '0', max: '359', value: '0' });
      const out = el('output', { textContent: '0°' });
      range.oninput = function () {
        img.style.transform = 'rotate(' + range.value + 'deg)';
        out.textContent = range.value + '°';
      };
      const go = el('button', { className: 'go', textContent: 'Send' });
      go.onclick = function () { ctx.submit({ kind: 'rotate', degrees: Number(range.value) }); };
      return el('div', {}, [
        el('div', { className: 'canvas' }, [img]),
        el('div', { className: 'row' }, [range, out, go]),
      ]);
    },

    hold: function (job, ctx) {
      const btn = el('button', { className: 'hold', textContent: 'Press and hold' });
      let started = 0;
      btn.addEventListener('pointerdown', function (e) {
        e.preventDefault();
        started = Date.now();
        btn.classList.add('on');
      });
      const release = function () {
        if (!started) return;
        const ms = Date.now() - started;
        started = 0;
        btn.classList.remove('on');
        ctx.submit({ kind: 'hold', ms: ms });
      };
      btn.addEventListener('pointerup', release);
      btn.addEventListener('pointercancel', release);
      btn.addEventListener('pointerleave', release);
      return el('div', {}, [el('div', { className: 'row' }, [btn])]);
    },

    // Not a challenge: a page, and the question "how much is actually in it".
    //
    // Four levels rather than a slider, and for a measured reason. FineWeb-Edu distilled a large
    // model's 0-5 judgements and published the confusion matrix: recall 0.35 at the second-highest
    // level and 0.01 at the highest. Finer resolution than four does not survive contact with
    // either a person or a model, so asking for it collects noise and stores it as a label.
    rate: function (job, ctx) {
      let payload = {};
      try {
        payload = JSON.parse(job.payload || '{}');
      } catch (e) {
        payload = {};
      }
      const kids = [];
      if (payload.url) {
        kids.push(el('div', { className: 'prompt', textContent: payload.url }));
      }
      // The text as the caller would have received it, so the judgement is about what svipall
      // actually returns rather than about how the page looks in a browser.
      kids.push(
        el('pre', {
          className: 'rate-text',
          textContent: String(payload.text || '').slice(0, 4000),
        })
      );
      const levels = [
        ['junk', 'Junk'],
        ['thin', 'Thin'],
        ['ordinary', 'Ordinary'],
        ['substantive', 'Substantive'],
      ];
      const row = el(
        'div',
        { className: 'row' },
        levels.map(function (pair) {
          const b = el('button', { textContent: pair[1] });
          b.addEventListener('click', function () {
            ctx.submit({ kind: 'rate', level: pair[0] });
          });
          return b;
        })
      );
      kids.push(row);
      return el('div', {}, kids);
    },
  };

  global.SVIPALL_MODES = MODES;
  // Exposed for the panel's own use and for anyone testing a renderer in isolation.
  global.SVIPALL_FRACTION = fraction;
})(window);
