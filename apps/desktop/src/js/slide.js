// First-run setup: when a step is left, a picture of it stays behind and slides
// out of the window while the next step slides in, so you can see it go.
if (!window.mpSlideOut) {
  window.mpSlideOut = (forward) => {
    const app = document.querySelector('.app');
    if (app && app.dataset.motion === 'snappy') return;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const root = document.querySelector('.onboard');
    const body = document.querySelector('.onboard-body');
    if (!root || !body) return;
    const r = body.getBoundingClientRect();
    const ghost = body.cloneNode(true);
    ghost.classList.remove('from-right', 'from-left');
    ghost.classList.add('ghost', forward ? 'out-left' : 'out-right');
    Object.assign(ghost.style, {
      position: 'fixed', left: r.left + 'px', top: r.top + 'px',
      width: r.width + 'px', height: r.height + 'px', margin: '0',
      zIndex: '20', pointerEvents: 'none',
    });
    root.appendChild(ghost);
    ghost.scrollTop = body.scrollTop;
    ghost.addEventListener('animationend', () => ghost.remove());
    setTimeout(() => ghost.remove(), 900);
  };
}
