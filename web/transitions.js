// 页面、弹窗和首页进入详情的临时动画；首页连续切换由 home-backdrop.js 独立管理。
const reducedMotion = matchMedia('(prefers-reduced-motion:reduce)');
let animations = [];
let temporary = [];
function clear() {
  animations.forEach(animation => animation.cancel());
  temporary.forEach(node => node.remove());
  animations = [];
  temporary = [];
}
function animate(node, frames, options) {
  if (!node?.animate) return;
  const animation = node.animate(frames, options);
  animations.push(animation);
  animation.finished.then(() => animation.cancel()).catch(() => {});
  return animation;
}
function decorativeClone(node) {
  if (!node) return null;
  const clone = node.cloneNode(true);
  clone.inert = true;
  clone.setAttribute('aria-hidden', 'true');
  [clone, ...clone.querySelectorAll('*')].forEach(n => {
    for (const name of ['id', 'role', 'data-action', 'data-input', 'data-submit']) n.removeAttribute(name);
  });
  clone.classList.add('transition-copy');
  return clone;
}
export function captureTransition() {
  const home = document.querySelector('.character-home');
  const visiblePortrait = home?.querySelector('.home-backdrop-layer:last-child .home-portrait');
  const previousPortrait = reducedMotion.matches ? null : decorativeClone(visiblePortrait);
  const portraitBounds = visiblePortrait?.getBoundingClientRect();
  clear();
  const modal = document.querySelector('.editor-overlay');
  return {
    page: document.querySelector('[data-page]')?.dataset.page,
    portrait: previousPortrait,
    portraitBounds,
    modalKey: modal?.querySelector('[role=dialog]')?.getAttribute('aria-labelledby'),
    modal: reducedMotion.matches ? null : decorativeClone(modal),
  };
}
export function playTransition(previous) {
  if (reducedMotion.matches) return;
  const page = document.querySelector('[data-page]');
  const modal = document.querySelector('.editor-overlay');
  const modalKey = modal?.querySelector('[role=dialog]')?.getAttribute('aria-labelledby');
  const ease = 'cubic-bezier(.22,1,.36,1)';
  if (page && page.dataset.page !== previous.page) {
    const frames = page.dataset.page === 'profile' ? [{opacity:0},{opacity:1}] : [{opacity:0,transform:'translateY(10px)'},{opacity:1,transform:'translateY(0)'}];
    animate(page, frames, {duration:260,easing:ease});
  }
  const detailPortrait = document.querySelector('.detail-art>img');
  if (previous.page === 'home' && page?.dataset.page === 'profile' && previous.portrait && detailPortrait && previous.portraitBounds?.width) {
    const flight = previous.portrait;
    const target = detailPortrait.getBoundingClientRect();
    flight.className = 'portrait-flight transition-copy';
    flight.style.visibility = '';
    document.body.append(flight);
    temporary.push(flight);
    const frame = rect => ({transform:`translate(${rect.left}px,${rect.top}px)`,width:`${rect.width}px`,height:`${rect.height}px`});
    const options = {duration:440,easing:'cubic-bezier(.22,.75,.2,1)'};
    const movement = animate(flight,[{...frame(previous.portraitBounds),opacity:1},{...frame(target),opacity:0}],options);
    movement?.finished.then(() => flight.remove()).catch(() => {});
    animate(detailPortrait,[{opacity:0},{opacity:1}],options);
  }
  if (modalKey && modalKey !== previous.modalKey) {
    animate(modal, [{opacity:0},{opacity:1}], {duration:180,easing:'ease-out'});
    // 库弹窗内的固定定位候选框使用视口坐标。
    animate(modal.querySelector('.editor-dialog:not(.library-dialog)'), [{transform:'translateY(12px) scale(.985)'},{transform:'translateY(0) scale(1)'}], {duration:240,easing:ease});
  } else if (!modal && previous.modal) {
    document.getElementById('app').append(previous.modal);
    temporary.push(previous.modal);
    const animation = animate(previous.modal,[{opacity:1},{opacity:0}],{duration:160,easing:'ease-out'});
    animation?.finished.then(() => previous.modal.remove()).catch(() => {});
  }
}
reducedMotion.addEventListener('change', clear);
