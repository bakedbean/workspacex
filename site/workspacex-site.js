// workspace-x.com — interactions: nav shadow, copy button, scroll reveal,
// one-time hero terminal typing sequence.

(function () {
  'use strict';
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // ── nav border on scroll ──────────────────────────────────────────────
  const nav = document.querySelector('.nav');
  const onScroll = () => nav && nav.classList.toggle('scrolled', window.scrollY > 8);
  onScroll();
  window.addEventListener('scroll', onScroll, { passive: true });

  // ── video lazy-src (avoids resource errors when files not present) ────────
  document.querySelectorAll('video[data-src]').forEach((v) => {
    const src = v.getAttribute('data-src');
    const ph = v.nextElementSibling;
    // Probe whether the file exists before committing src
    const xhr = new XMLHttpRequest();
    xhr.open('HEAD', src, true);
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 400) {
        v.src = src;
        if (ph && ph.classList.contains('cast-ph')) ph.style.display = 'none';
      }
    };
    xhr.send();
  });

  // ── copy buttons ──────────────────────────────────────────────────────
  document.querySelectorAll('[data-copy]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const text = btn.getAttribute('data-copy');
      try { await navigator.clipboard.writeText(text); } catch (e) {}
      btn.classList.add('copied');
      const orig = btn.innerHTML;
      btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6L9 17l-5-5"/></svg>';
      setTimeout(() => { btn.classList.remove('copied'); btn.innerHTML = orig; }, 1400);
    });
  });

  // ── scroll reveal ─────────────────────────────────────────────────────
  const reveals = document.querySelectorAll('.reveal');
  if (reduce || !('IntersectionObserver' in window)) {
    reveals.forEach((el) => el.classList.add('in'));
  } else {
    const io = new IntersectionObserver((entries) => {
      entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add('in'); io.unobserve(e.target); } });
    }, { threshold: 0.12, rootMargin: '0px 0px -8% 0px' });
    reveals.forEach((el) => io.observe(el));

    // Safety net: reveal anything already in (or near) the viewport on load —
    // some embedded/preview iframes don't deliver the initial IO callback —
    // and guarantee nothing stays hidden permanently.
    const sweep = () => reveals.forEach((el) => {
      if (el.getBoundingClientRect().top < window.innerHeight * 0.96) el.classList.add('in');
    });
    requestAnimationFrame(sweep);
    setTimeout(sweep, 200);
    setTimeout(() => reveals.forEach((el) => el.classList.add('in')), 2600);
  }

  // ── hero terminal typing ──────────────────────────────────────────────
  const typeHost = document.getElementById('demo-type');
  const dash = document.getElementById('hero-dash');
  if (typeHost) {
    const lines = [
      { pfx: '$ ', txt: 'cargo build --release', out: '   Compiling wsx v0.4.2\n   Finished release [optimized] target(s)' },
      { pfx: '$ ', txt: './target/release/wsx repo add ~/work/api', out: '   added 1 repo · create worktrees in-app' },
      { pfx: '$ ', txt: './target/release/wsx', out: '   launching dashboard…' },
    ];
    if (reduce) {
      typeHost.innerHTML = lines.map((l) =>
        `<span class="ln"><span class="pfx">${l.pfx}</span>${l.txt}</span>` +
        (l.out ? `<span class="ln out">${l.out}</span>` : '')
      ).join('');
      if (dash) dash.style.display = '';
    } else {
      if (dash) dash.style.display = 'none';
      runType(typeHost, lines, () => {
        if (dash) { dash.style.display = ''; dash.classList.add('in'); }
      });
    }
  }

  function runType(host, lines, done) {
    host.innerHTML = '';
    let li = 0;
    function nextLine() {
      if (li >= lines.length) { done && done(); return; }
      const l = lines[li];
      const span = document.createElement('span');
      span.className = 'ln';
      const pfx = document.createElement('span'); pfx.className = 'pfx'; pfx.textContent = l.pfx;
      span.appendChild(pfx);
      const body = document.createElement('span'); span.appendChild(body);
      const cur = document.createElement('span'); cur.className = 'cursor'; cur.textContent = '█';
      span.appendChild(cur);
      host.appendChild(span);
      let ci = 0;
      const tick = () => {
        if (ci <= l.txt.length) {
          body.textContent = l.txt.slice(0, ci);
          ci++;
          setTimeout(tick, 26 + Math.random() * 34);
        } else {
          cur.remove();
          if (l.out) {
            const o = document.createElement('span'); o.className = 'ln out'; o.textContent = l.out;
            host.appendChild(o);
          }
          li++;
          setTimeout(nextLine, 360);
        }
      };
      setTimeout(tick, 90);
    }
    nextLine();
  }
})();
