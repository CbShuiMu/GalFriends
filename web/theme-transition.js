// GalGear 的主题扩散与旧 WebView 淡出回退；这里只维护瞬时动画，不改变主题状态。
import {dismissNotice} from './progress-notice.js';
let generation = 0;
let activeTransition = null;
let fallbackTimer;
const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');

function clearFallback() {
  clearTimeout(fallbackTimer);
  document.querySelector('.theme-fallback')?.remove();
}

// Rust 生成的片段只包含主题控件；保留页面、图片、滚动位置和输入节点。
export function applyThemePatch(html) {
  const template = document.createElement('template');
  template.innerHTML = html;
  const patch = template.content.firstElementChild;
  for (const target of [document.documentElement, document.querySelector('.app-shell')]) {
    if (!target) continue;
    target.dataset.theme = patch.dataset.theme;
    target.dataset.colorTheme = patch.dataset.colorTheme;
  }
  for (const next of patch.querySelectorAll('button[data-action]')) {
    const current = [...document.querySelectorAll(`button[data-action="${next.dataset.action}"]`)]
      .find(button => button.dataset.value === next.dataset.value);
    if (!current) continue;
    for (const attribute of [...current.attributes]) {
      if (!next.hasAttribute(attribute.name)) current.removeAttribute(attribute.name);
    }
    for (const attribute of next.attributes) {
      if (current.getAttribute(attribute.name) !== attribute.value) current.setAttribute(attribute.name, attribute.value);
    }
    if (current.innerHTML !== next.innerHTML) current.innerHTML = next.innerHTML;
  }
  document.querySelectorAll('#app .notice-stack > [data-notice-scope="app"]')
    .forEach(node => dismissNotice(node));
}

export function rememberThemeOrigin(event, button) {
  if (!button || !['theme', 'settings-theme', 'settings-color_theme'].includes(button.dataset.action)) return;
  const rect = button.getBoundingClientRect();
  const pointer = event.type === 'click' && event.detail > 0;
  document.documentElement.style.setProperty('--theme-x', `${pointer ? event.clientX : rect.x + rect.width / 2}px`);
  document.documentElement.style.setProperty('--theme-y', `${pointer ? event.clientY : rect.y + rect.height / 2}px`);
}

export function renderWithTheme(html, render) {
  const current = ++generation;
  activeTransition?.skipTransition();
  activeTransition = null;
  clearFallback();
  const theme = /data-theme="(light|dark)"/.exec(html)?.[1];
  const color = /data-color-theme="(pink|white-pink|white)"/.exec(html)?.[1];
  const previous = document.documentElement.dataset;
  // 后续响应可能先于截图回调到达，不能让旧 HTML 覆盖最新界面。
  const apply = () => { if (current === generation) render(html); };
  if (!previous.theme || !theme || (previous.theme === theme && previous.colorTheme === color) || reducedMotion.matches) {
    apply();
    return;
  }
  if (typeof document.startViewTransition === 'function') {
    const transition = document.startViewTransition(apply);
    activeTransition = transition;
    transition.ready.catch(() => {});
    transition.finished.catch(() => {}).finally(() => {
      if (activeTransition === transition) activeTransition = null;
    });
    return;
  }
  const old = document.querySelector('.app-shell')?.cloneNode(true);
  const scroll = document.querySelector('.page-scroll')?.scrollTop || 0;
  if (old) {
    old.classList.add('theme-fallback');
    old.inert = true;
    old.setAttribute('aria-hidden', 'true');
    for (const node of [old, ...old.querySelectorAll('*')]) {
      for (const key of ['id', 'data-action', 'data-input', 'data-submit']) node.removeAttribute(key);
    }
  }
  apply();
  if (old) {
    document.body.append(old);
    const pane = old.querySelector('.page-scroll');
    if (pane) pane.scrollTop = scroll;
    old.addEventListener('animationend', () => old.remove(), {once:true});
    fallbackTimer = setTimeout(() => old.remove(), 600);
  }
}

reducedMotion.addEventListener('change', () => {
  activeTransition?.skipTransition();
  clearFallback();
});
