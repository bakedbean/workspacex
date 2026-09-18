// workspace-x.com — nav shadow, lazy video, copy button, lightbox, scroll reveal.
(function () {
  'use strict';
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // nav border on scroll
  const nav = document.querySelector('.nav');
  const onScroll = () => nav && nav.classList.toggle('scrolled', window.scrollY > 8);
  onScroll();
  window.addEventListener('scroll', onScroll, { passive: true });

  // only attach video sources that actually exist, so the placeholder stays
  // visible (and the console stays clean) until the mp4s are dropped in assets/
  document.querySelectorAll('video[data-src]').forEach((v) => {
    const src = v.getAttribute('data-src');
    const ph = v.nextElementSibling;
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

  // copy button — async clipboard API, with an execCommand fallback for
  // insecure contexts; the checkmark only shows when one of them succeeded
  function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      return navigator.clipboard.writeText(text).then(() => true, () => copyLegacy(text));
    }
    return Promise.resolve(copyLegacy(text));
  }
  function copyLegacy(text) {
    const ta = document.createElement('textarea');
    ta.value = text; ta.setAttribute('readonly', ''); ta.style.position = 'fixed'; ta.style.opacity = '0';
    document.body.appendChild(ta); ta.select();
    let ok = false;
    try { ok = document.execCommand('copy'); } catch (e) {}
    ta.remove();
    return ok;
  }
  document.querySelectorAll('[data-copy]').forEach((btn) => {
    const orig = btn.innerHTML;
    btn.addEventListener('click', async () => {
      if (!(await copyText(btn.getAttribute('data-copy')))) return;
      btn.classList.add('copied');
      btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6L9 17l-5-5"/></svg>';
      setTimeout(() => { btn.classList.remove('copied'); btn.innerHTML = orig; }, 1400);
    });
  });

  // lightbox — each .thumb opens the full-size still named by its data-* in
  // the one <dialog>; the sources are swapped in on open and cleared on close
  // so a closed dialog holds no image.
  const box = document.getElementById('lightbox');
  if (box && typeof box.showModal === 'function') {
    const src = box.querySelector('source');
    const img = box.querySelector('img');
    const cap = box.querySelector('.lightbox-cap');
    document.querySelectorAll('.thumb[data-full]').forEach((btn) => {
      btn.addEventListener('click', () => {
        const thumbImg = btn.querySelector('img');
        src.srcset = btn.getAttribute('data-full-webp') || '';
        img.src = btn.getAttribute('data-full');
        img.alt = thumbImg ? thumbImg.alt.replace(/\s*Opens full size\.$/, '') : '';
        cap.textContent = btn.getAttribute('data-caption') || '';
        box.showModal();
      });
    });
    box.querySelector('[data-close]').addEventListener('click', () => box.close());
    // a click on the backdrop lands on the dialog element itself, not its children
    box.addEventListener('click', (e) => { if (e.target === box) box.close(); });
    box.addEventListener('close', () => { src.srcset = ''; img.removeAttribute('src'); img.alt = ''; });
  }

  // scroll reveal
  const reveals = document.querySelectorAll('.reveal');
  if (reduce || !('IntersectionObserver' in window)) {
    reveals.forEach((el) => el.classList.add('in'));
  } else {
    const io = new IntersectionObserver((entries) => {
      entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add('in'); io.unobserve(e.target); } });
    }, { threshold: 0.12, rootMargin: '0px 0px -8% 0px' });
    reveals.forEach((el) => io.observe(el));
    const sweep = () => reveals.forEach((el) => {
      if (el.getBoundingClientRect().top < window.innerHeight * 0.96) el.classList.add('in');
    });
    requestAnimationFrame(sweep);
    setTimeout(sweep, 200);
    setTimeout(() => reveals.forEach((el) => el.classList.add('in')), 2600);
  }
})();
