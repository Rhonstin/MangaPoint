const root = document.getElementById('editor-source-art');
if (root && !root.dataset.cropBound) {
  root.dataset.cropBound='1';
  let drag=null;
  root.addEventListener('pointerdown', e => {
    const handle=e.target.closest('[data-crop-handle]');
    if (!handle || !root.classList.contains('is-editing')) return;
    const input=document.getElementById('editor-crop-draft');
    const edges=input.value.split(',').map(Number);
    drag={edge:handle.dataset.cropHandle, x:e.clientX, y:e.clientY, bounds:root.getBoundingClientRect(), edges};
    root.setPointerCapture(e.pointerId); e.preventDefault();
  });
  root.addEventListener('pointermove', e => {
    if (!drag) return;
    const dx=(e.clientX-drag.x)/drag.bounds.width, dy=(e.clientY-drag.y)/drag.bounds.height;
    let [l,t,r,b]=drag.edges; r=1-r; b=1-b;
    if (drag.edge==='move') {
      const mx=Math.max(-l,Math.min(1-r,dx)), my=Math.max(-t,Math.min(1-b,dy));
      l+=mx; r+=mx; t+=my; b+=my;
    } else {
      if (drag.edge.includes('w')) l=Math.max(0,Math.min(r-.01,l+dx));
      if (drag.edge.includes('e')) r=Math.min(1,Math.max(l+.01,r+dx));
      if (drag.edge.includes('n')) t=Math.max(0,Math.min(b-.01,t+dy));
      if (drag.edge.includes('s')) b=Math.min(1,Math.max(t+.01,b+dy));
    }
    const box=root.querySelector('.editor-crop-box');
    Object.assign(box.style,{left:(l*100)+'%',top:(t*100)+'%',width:((r-l)*100)+'%',height:((b-t)*100)+'%'});
    const input=document.getElementById('editor-crop-draft');
    const value=[l,t,1-r,1-b].join(',');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,value);
    input.dispatchEvent(new Event('input',{bubbles:true}));
  });
  const stop=()=>{drag=null;};
  root.addEventListener('pointerup',stop); root.addEventListener('pointercancel',stop);
}
