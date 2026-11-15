// 复用 GalGear 的卡片对齐和输入导航；角色选择仍由 Rust 管理。
import {topLayer} from './navigation.js';
import {syncHomeBackdrop} from './home-backdrop.js';

let selectedGame = null;
let observedShelf = null;
let shelfObserver = null;
let alignmentFrame = 0;
const reducedMotion = matchMedia('(prefers-reduced-motion:reduce)');

function updateShelfTail(shelf) {
  const last = shelf.lastElementChild;
  if (last?.matches('.home-game')) {
    const width = `${last.getBoundingClientRect().width}px`;
    if (shelf.style.getPropertyValue('--home-last-width') !== width) {
      shelf.style.setProperty('--home-last-width', width);
    }
  }
}

function alignSelection(game, animate) {
  cancelAnimationFrame(alignmentFrame);
  alignmentFrame = 0;
  if (!game) return;
  const shelf = game.parentElement;
  const from = shelf.scrollLeft;
  const started = performance.now();
  const duration = animate && !reducedMotion.matches
    ? Number(getComputedStyle(shelf).getPropertyValue('--home-motion-ms')) : 0;
  const step = now => {
    alignmentFrame = 0;
    if (!game.isConnected || !game.classList.contains('selected')) return;
    // Earlier posters shrink during the same transition, so follow their live
    // positions rather than scrolling to a stale offset and snapping at the end.
    const inset = parseFloat(getComputedStyle(shelf).paddingLeft) || 0;
    const target = shelf.scrollLeft + game.getBoundingClientRect().left - shelf.getBoundingClientRect().left - inset;
    const progress = duration ? Math.min(1, (now - started) / duration) : 1;
    // Read all geometry before extending the scroll range, avoiding a second
    // forced layout in the same animation frame. The target is in shelf coordinates.
    updateShelfTail(shelf);
    shelf.scrollLeft = from + (target - from) * (1 - (1 - progress) ** 3);
    if (progress < 1) alignmentFrame = requestAnimationFrame(step);
  };
  if (duration) alignmentFrame = requestAnimationFrame(step);
  else step(started);
}

export function syncHomePresentation() {
  const shelf = document.querySelector('.home-shelf');
  if (shelf !== observedShelf) {
    shelfObserver?.disconnect();
    observedShelf = shelf;
    selectedGame = null;
    cancelAnimationFrame(alignmentFrame);
    alignmentFrame = 0;
    if (shelf) shelfObserver?.observe(shelf);
  }
  const game = shelf?.querySelector('.home-game.selected') || null;
  const games = shelf && game !== selectedGame ? [...shelf.children] : [];
  const direction = game && selectedGame?.isConnected
    ? Math.sign(games.indexOf(game) - games.indexOf(selectedGame)) : 0;
  syncHomeBackdrop(document.querySelector('.home-backdrop'), direction);
  if (game !== selectedGame) {
    const animate = !!selectedGame?.isConnected;
    selectedGame = game;
    alignSelection(game, animate);
  }
  if (shelf) updateShelfTail(shelf);
}

export function installHomeNavigation(dispatch, ready) {
  const select = game => {
    // Focus can be restored during a Rust render, so dispatch after that borrow ends.
    queueMicrotask(() => {
      if (ready() && game?.isConnected && document.activeElement === game) {
        if (game.getAttribute('aria-pressed') !== 'true') dispatch('select', game.dataset.value);
      }
    });
  };
  shelfObserver = new ResizeObserver(() => {
    // Responsive heights also transition; follow the moving offset until they settle.
    alignSelection(selectedGame, true);
  });
  if (observedShelf) shelfObserver.observe(observedShelf);
  document.addEventListener('load', event => {
    if (event.target.matches?.('.home-game img') && !alignmentFrame) alignSelection(selectedGame, false);
  }, true);
  document.addEventListener('focusin', event => {
    const game = event.target.closest?.('.home-game');
    if (game) select(game);
  });
  // Focus on click with preventScroll so the browser cannot jump ahead of the shelf animation.
  document.addEventListener('pointerdown', event => {
    if (event.target.closest?.('.home-game')) event.preventDefault();
  });
  document.addEventListener('click', event => {
    const game = event.target.closest?.('.home-game');
    if (event.detail > 0 && game) {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (activeHome() && !game.disabled) {
        game.focus({preventScroll:true});
        select(game);
      }
    }
  }, true);

  const activeHome = () => ready() && topLayer()?.matches('.app-shell') &&
    document.querySelector('.character-home:not([inert])') && !document.querySelector('.character-home')?.closest('[inert]');
  const move = direction => {
    const games = [...document.querySelectorAll('.home-shelf .home-game')];
    // Rapid wheel events can arrive before the focus-triggered render finishes.
    const focused = games.indexOf(document.activeElement);
    const index = Math.max(0, focused < 0 ? games.indexOf(selectedGame) : focused);
    const next = direction === 'first' ? 0 : direction === 'last' ? games.length - 1
      : Math.max(0, Math.min(games.length - 1, index + direction));
    games[next]?.focus({preventScroll:true});
  };
  document.addEventListener('keydown', event => {
    if (event.defaultPrevented || event.isComposing || event.altKey || event.ctrlKey || event.metaKey || !activeHome()) return;
    if (event.target.closest?.('input,textarea,select,[contenteditable="true"]')) return;
    const direction = {ArrowLeft:-1, ArrowRight:1, Home:'first', End:'last'}[event.key];
    if (direction === undefined) return;
    event.preventDefault();
    move(direction);
  });
  let wheelDelta = 0, lastWheelEvent = -Infinity;
  document.addEventListener('wheel', event => {
    if (!activeHome() || !event.target.closest?.('.character-home')) return;
    event.preventDefault();
    const shelf = document.querySelector('.home-shelf');
    if (!shelf) return;
    const now = performance.now();
    const scale = event.deltaMode === 1 ? 24 : event.deltaMode === 2 ? shelf.clientWidth : 1;
    const delta = (event.deltaY || event.deltaX) * scale;
    const direction = Math.sign(delta);
    if (!direction) return;
    if (now - lastWheelEvent > 200 || direction !== Math.sign(wheelDelta)) wheelDelta = 0;
    lastWheelEvent = now;
    wheelDelta += delta;
    if (Math.abs(wheelDelta) < 40) return;
    // Consume each wheel step immediately; a cooldown drops fast consecutive notches.
    move(direction);
    wheelDelta = 0;
  }, {passive:false});
}

reducedMotion.addEventListener('change', () => alignSelection(selectedGame, false));
