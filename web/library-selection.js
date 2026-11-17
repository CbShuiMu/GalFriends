// 在 DOM 中预览选框，松开时将角色和收藏夹 ID 一次提交给 Rust。
const cards = '[data-character-id], [data-folder-id]';
const blocked = '.library-bar,.folder-breadcrumbs,.game-card,.folder-card,button,a,input,select,textarea';

export function installLibrarySelection(commit) {
  let drag = null;
  let frame = 0;

  function paint(scrollAtEdge) {
    const { pane, box, x, y, startX, startY, entries, additive } = drag;
    const bounds = pane.getBoundingClientRect();
    const rightEdge = bounds.left + pane.clientWidth;
    const bottomEdge = bounds.top + pane.clientHeight;
    const previousScroll = pane.scrollTop;
    if (scrollAtEdge) {
      if (y > bottomEdge - 28) pane.scrollTop += 12;
      else if (y < bounds.top + 28) pane.scrollTop -= 12;
    }

    const originY = startY - pane.scrollTop;
    const endX = Math.max(bounds.left, Math.min(x, rightEdge));
    const endY = Math.max(bounds.top, Math.min(y, bottomEdge));
    const left = Math.min(startX, endX), right = Math.max(startX, endX);
    const top = Math.min(originY, endY), bottom = Math.max(originY, endY);
    const visibleTop = Math.max(top, bounds.top);
    const visibleBottom = Math.min(bottom, bottomEdge);
    Object.assign(box.style, {
      left: `${left}px`, top: `${visibleTop}px`,
      width: `${right - left}px`, height: `${Math.max(0, visibleBottom - visibleTop)}px`
    });
    const selection = { characters: [], folders: [] };
    for (const entry of entries) {
      const rect = entry.card.getBoundingClientRect();
      const intersects = rect.left < right && rect.right > left && rect.top < bottom && rect.bottom > top;
      const selected = (additive && entry.original) || intersects;
      entry.card.classList.toggle('is-selected', selected);
      if (selected) selection[entry.kind].push(entry.id);
    }
    drag.selection = selection;
    if (scrollAtEdge && pane.scrollTop !== previousScroll) schedulePaint();
  }

  function schedulePaint() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      paint(true);
    });
  }

  function cancel() {
    if (!drag) return;
    const { pane, box, pointerId, entries } = drag;
    drag = null;
    cancelAnimationFrame(frame);
    frame = 0;
    if (!box) return;
    box.remove();
    pane.classList.remove('marquee-selecting');
    for (const entry of entries) entry.card.classList.toggle('is-selected', entry.original);
    if (pane.hasPointerCapture(pointerId)) pane.releasePointerCapture(pointerId);
  }

  document.addEventListener('pointerdown', event => {
    if (drag || !event.isPrimary || event.button !== 0 || event.pointerType === 'touch'
      || event.altKey || event.target.closest(blocked)) return;
    const pane = event.target.closest('.page-scroll:not([inert])');
    const page = pane?.querySelector('.library-page');
    const toggle = page?.querySelector('[data-action=batch-toggle]');
    if (!toggle || toggle.disabled || event.clientX >= pane.getBoundingClientRect().left + pane.clientWidth) return;
    const entries = [...page.querySelectorAll(cards)].map(card => ({
      card,
      id: card.dataset.characterId ?? card.dataset.folderId,
      kind: card.hasAttribute('data-character-id') ? 'characters' : 'folders',
      original: card.classList.contains('is-selected')
    }));
    if (!entries.length) return;
    drag = {
      pane, entries, pointerId: event.pointerId, startX: event.clientX,
      startY: event.clientY + pane.scrollTop, additive: event.ctrlKey || event.shiftKey || event.metaKey
    };
  });

  document.addEventListener('selectstart', event => {
    if (drag) event.preventDefault();
  });
  document.addEventListener('pointermove', event => {
    if (event.pointerId !== drag?.pointerId) return;
    drag.x = event.clientX;
    drag.y = event.clientY;
    if (!drag.box) {
      if (Math.hypot(drag.x - drag.startX, drag.y + drag.pane.scrollTop - drag.startY) < 6) return;
      drag.pane.setPointerCapture(event.pointerId);
      drag.pane.classList.add('marquee-selecting');
      drag.box = document.createElement('div');
      drag.box.className = 'marquee-box';
      drag.box.setAttribute('aria-hidden', 'true');
      drag.pane.closest('.app-shell').append(drag.box);
      document.getSelection()?.removeAllRanges();
    }
    event.preventDefault();
    schedulePaint();
  });
  document.addEventListener('scroll', event => {
    if (drag?.box && drag.pane === event.target) schedulePaint();
  }, true);
  document.addEventListener('pointerup', event => {
    if (event.pointerId !== drag?.pointerId) return;
    if (!drag.box) {
      cancel();
      return;
    }
    drag.x = event.clientX;
    drag.y = event.clientY;
    paint(false);
    const selection = drag.selection;
    event.preventDefault();
    cancel();
    commit(JSON.stringify(selection));
  });
  for (const type of ['pointercancel', 'lostpointercapture']) {
    document.addEventListener(type, event => {
      if (event.pointerId === drag?.pointerId) cancel();
    });
  }
  document.addEventListener('keydown', event => {
    if (event.key !== 'Escape' || !drag) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    cancel();
  }, true);
  window.addEventListener('blur', cancel);
  return cancel;
}
