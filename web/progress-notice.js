// 只维护通知动画与 DOM 生命周期；业务阶段与本地化内容由 Rust 提供。
const transitions = new WeakMap();
const noticeAnimations = new Map();
const motion = matchMedia('(prefers-reduced-motion: reduce)');
motion.addEventListener('change', () => {
  if (motion.matches) {
    noticeAnimations.forEach(animation => animation.finish());
    document.querySelectorAll('.progress-notice-viewport').forEach(viewport => {
      viewport.getAnimations({subtree: true}).forEach(animation => animation.finish());
    });
  }
});

function setText(notice, text) {
  const viewport = notice.querySelector('.progress-notice-viewport');
  if (!viewport) return;
  let state = transitions.get(notice);
  if (!state) {
    state = {text: viewport.textContent, pending: null, running: false};
    transitions.set(notice, state);
  }
  // 快速到达的阶段合并为最新一条，避免已完成后仍播放过时进度。
  state.pending = text;
  if (state.running) return;
  if (state.text === text) {
    state.pending = null;
    return;
  }
  const previous = viewport.querySelector('.progress-notice-line');
  const next = document.createElement('span');
  next.className = 'progress-notice-line';
  next.textContent = text;
  viewport.title = text;
  state.text = text;
  state.pending = null;
  if (motion.matches || !previous?.animate) {
    viewport.replaceChildren(next);
    return;
  }
  state.running = true;
  previous.setAttribute('aria-hidden', 'true');
  previous.classList.add('progress-notice-outgoing');
  viewport.append(next);
  const options = {duration: 300, easing: 'cubic-bezier(.22,1,.36,1)', fill: 'both'};
  const outgoing = previous.animate([
    {transform: 'translateY(0)', opacity: 1},
    {transform: 'translateY(-100%)', opacity: 0},
  ], options);
  const incoming = next.animate([
    {transform: 'translateY(100%)', opacity: 0},
    {transform: 'translateY(0)', opacity: 1},
  ], options);
  Promise.allSettled([outgoing.finished, incoming.finished]).then(() => {
    previous.remove();
    outgoing.cancel();
    incoming.cancel();
    state.running = false;
    if (notice.isConnected && state.pending !== null) setText(notice, state.pending);
  });
}

export function renderProgressNotice(key, text) {
  const notice = [...document.querySelectorAll('#app .notice-stack > [data-progress-notice]')]
    .find(node => node.dataset.progressNotice === key);
  if (notice) setText(notice, text);
}

function noticeKey(node) {
  if (node.dataset.progressNotice) return `progress:${node.dataset.progressNotice}`;
  if (node.dataset.noticeScope) return `${node.dataset.noticeScope}:${node.dataset.noticeId}`;
  return `status:${node.className}:${node.textContent}`;
}

function captureNotice(node) {
  return {node, key: noticeKey(node), bounds: node.getBoundingClientRect(), opacity: getComputedStyle(node).opacity};
}

export function captureNotices() {
  return [...document.querySelectorAll('#app .notice-stack > .notice')].map(captureNotice);
}

function animateNotice(node, frames, options, complete = () => {}) {
  const animation = node.animate(frames, {...options, fill: 'both'});
  noticeAnimations.set(node, animation);
  const finish = () => {
    if (noticeAnimations.get(node) === animation) noticeAnimations.delete(node);
    complete();
    animation.cancel();
  };
  animation.finished.then(finish, finish);
}

// 重绘保留同一消息的节点，入场动画和自动关闭倒计时都不重新开始。
export function restoreNotices(previousNotices) {
  const previous = new Map(previousNotices.map(snapshot => [snapshot.key, snapshot]));
  for (const next of document.querySelectorAll('#app .notice-stack > .notice')) {
    const key = noticeKey(next);
    const snapshot = previous.get(key);
    if (snapshot) {
      previous.delete(key);
      const node = snapshot.node;
      if (node === next) continue;
      next.replaceWith(node);
      if (next.dataset.progressNotice) {
        setText(node, next.querySelector('.progress-notice-viewport').textContent);
      } else if (node.innerHTML !== next.innerHTML) {
        node.innerHTML = next.innerHTML;
      }
    } else if (!motion.matches && next.animate) {
      animateNotice(next, [
        {transform: 'translateY(12px)', opacity: 0},
        {transform: 'translateY(0)', opacity: 1},
      ], {duration: 280, easing: 'cubic-bezier(.22,1,.36,1)'});
    }
  }
  previous.forEach(snapshot => dismissNotice(snapshot.node, snapshot));
}

export function dismissNotice(node, snapshot = captureNotice(node)) {
  const state = transitions.get(node);
  if (state) state.pending = null;
  noticeAnimations.get(node)?.cancel();
  const remove = () => {
    node.remove();
    node.querySelector('.progress-notice-viewport')?.getAnimations({subtree: true})
      .forEach(animation => animation.cancel());
  };
  if (motion.matches || !node.animate || !snapshot.bounds.width) {
    remove();
    return;
  }
  // 业务状态已经结束；临时保留无交互、无播报的视觉层，整页重绘也不会打断退场。
  const {left, top, width, height} = snapshot.bounds;
  node.inert = true;
  node.setAttribute('aria-hidden', 'true');
  for (const attribute of ['role', 'aria-live', 'data-progress-notice', 'data-notice-scope', 'data-notice-id', 'data-notice-duration']) {
    node.removeAttribute(attribute);
  }
  node.classList.add('notice-leaving');
  Object.assign(node.style, {
    left: `${left}px`, top: `${top}px`, width: `${width}px`, height: `${height}px`,
    right: 'auto', bottom: 'auto',
  });
  document.body.append(node);
  animateNotice(node, [{opacity: snapshot.opacity}, {opacity: 0}], {duration: 200, easing: 'ease-out'}, remove);
}

function removeEmptyStack() {
  const stack = document.querySelector('#app .notice-stack');
  if (stack && !stack.children.length) stack.remove();
}

const noticeTimers = new Map();

// 同一消息跨重绘沿用计时器；到期先核对 Rust 状态，再淡出对应 DOM。
export function syncNoticeTimers(expire) {
  const notices = [...document.querySelectorAll('#app .notice-stack > [data-notice-scope]')];
  const active = new Set(notices.map(node => `${node.dataset.noticeScope}:${node.dataset.noticeId}`));
  for (const [key, timer] of noticeTimers) {
    if (!active.has(key)) {
      clearTimeout(timer);
      noticeTimers.delete(key);
    }
  }
  for (const node of notices) {
    const {noticeScope: scope, noticeId: id, noticeDuration: duration} = node.dataset;
    const key = `${scope}:${id}`;
    if (noticeTimers.has(key)) continue;
    const timer = setTimeout(() => {
      noticeTimers.delete(key);
      if (!expire(scope, Number(id))) return;
      document.querySelectorAll('#app .notice-stack > [data-notice-scope]').forEach(current => {
        if (current.dataset.noticeScope === scope && current.dataset.noticeId === id) dismissNotice(current);
      });
      removeEmptyStack();
    }, Number(duration));
    noticeTimers.set(key, timer);
  }
  removeEmptyStack();
}
