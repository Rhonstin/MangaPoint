// The bars above the page (downloads, an export) stay in view while the page
// scrolls. Whatever else sticks to the top needs to know how tall they are, so
// the height is kept in --bars-h.
if (!window.mpBarsReady) {
  window.mpBarsReady = true;
  let queued = false;
  const apply = () => {
    queued = false;
    const bars = document.querySelector('.bars');
    const height = bars ? Math.round(bars.getBoundingClientRect().height) : 0;
    document.documentElement.style.setProperty('--bars-h', height + 'px');
  };
  const soon = () => {
    if (!queued) {
      queued = true;
      requestAnimationFrame(apply);
    }
  };
  new MutationObserver(soon).observe(document.body, { childList: true, subtree: true, characterData: true });
  window.addEventListener('resize', soon);
  apply();
}
