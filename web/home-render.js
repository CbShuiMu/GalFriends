// 首页沿用 GalGear 的稳定 DOM：卡片尺寸过渡、滚动和背景解码不能被 Rust 重绘打断。
function syncAttributes(current, next) {
  for (const attribute of [...current.attributes]) {
    if (attribute.name !== 'style' && !next.hasAttribute(attribute.name)) current.removeAttribute(attribute.name);
  }
  for (const attribute of next.attributes) {
    if (current.getAttribute(attribute.name) !== attribute.value) current.setAttribute(attribute.name, attribute.value);
  }
}

function syncShelf(current, next) {
  const cards = new Map([...current.children].map(card => [card.dataset.value, card]));
  let cursor = current.firstElementChild;
  for (const desired of [...next.children]) {
    const card = cards.get(desired.dataset.value);
    if (!card) {
      current.insertBefore(desired, cursor);
      continue;
    }
    cards.delete(desired.dataset.value);
    if (card !== cursor) current.insertBefore(card, cursor);
    syncAttributes(card, desired);
    // 当前选中立绘没有变化时保留图片节点，避免懒加载与尺寸动画重新开始。
    const oldImage = card.querySelector('img');
    const newImage = desired.querySelector('img');
    if (!oldImage || !newImage || oldImage.getAttribute('src') !== newImage.getAttribute('src')) {
      card.replaceChildren(...desired.childNodes);
    }
    cursor = card.nextElementSibling;
  }
  cards.forEach(card => card.remove());
}

function branchKey(node) {
  return ['app-shell', 'main-panel', 'page-scroll', 'character-home', 'home-backdrop', 'home-shelf']
    .find(name => node.classList.contains(name));
}

function syncBranch(current, next) {
  if (next.nodeType === Node.ELEMENT_NODE) syncAttributes(current, next);
  if (current.matches('.home-backdrop')) return;
  if (current.matches('.home-shelf')) {
    syncShelf(current, next);
    return;
  }
  // 只保留通向首页的容器与动画宿主；其他页面、表单和弹窗仍沿用既有渲染方式。
  const retained = new Map([...current.children]
    .filter(node => node.matches('.character-home,.home-backdrop,.home-shelf') || node.querySelector('.character-home'))
    .map(node => [branchKey(node), node]));
  let cursor = current.firstChild;
  for (const desired of [...next.childNodes]) {
    const match = desired.nodeType === Node.ELEMENT_NODE && retained.get(branchKey(desired));
    if (match) {
      while (cursor && cursor !== match) {
        const obsolete = cursor;
        cursor = cursor.nextSibling;
        obsolete.remove();
      }
      syncBranch(match, desired);
      cursor = match.nextSibling;
    } else {
      current.insertBefore(desired, cursor);
    }
  }
  while (cursor) {
    const obsolete = cursor;
    cursor = cursor.nextSibling;
    obsolete.remove();
  }
}

export function renderHomeFrame(root, html) {
  if (!root.querySelector('.character-home')) return false;
  const template = document.createElement('template');
  template.innerHTML = html;
  if (!template.content.querySelector('.character-home')) return false;
  syncBranch(root, template.content);
  return true;
}
