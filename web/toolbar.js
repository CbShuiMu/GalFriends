// GalGear's expanding add control; this module only owns DOM focus and disclosure animation.
import {positionChoice, moveChoiceFocus} from './choices.js';
export function setAddMenuExpanded(menu, expanded) {
  if (!menu) return;
  menu.dataset.expanded = String(expanded);
  menu.querySelector('.add-trigger').setAttribute('aria-expanded', String(expanded));
  menu.querySelector('.add-options').inert = !expanded;
}
export function captureToolbar() {
  return {expanded:document.querySelector('.add-menu')?.dataset.expanded === 'true',sortOpen:!!document.querySelector('#sort-options'),sortFocused:!!document.activeElement?.closest('#sort-options')};
}
export function syncToolbar(previous) {
  const menu=document.querySelector('.add-menu');
  setAddMenuExpanded(menu, previous.expanded || !!menu?.contains(document.activeElement));
  const options = document.querySelector('#sort-options');
  positionSort();
  if (options && !previous.sortOpen) options.querySelector('[aria-selected=true]')?.focus();
  if (!options && previous.sortOpen && previous.sortFocused) document.querySelector('#library-sort')?.focus();
}
function positionSort() {
  const trigger=document.querySelector('#library-sort'),popup=document.querySelector('#sort-options');
  positionChoice(trigger,popup);
}

export function positionTagSearch() {
  const search = document.activeElement?.closest('.tag-search');
  const input = search?.querySelector('input');
  const results = search?.querySelector('.tag-search-results');
  if (!results) return;
  // GalGear 的候选浮层固定定位，避免被弹窗正文裁切。
  const rect = input.getBoundingClientRect();
  const limit = Math.min(240, innerHeight * .35);
  const below = innerHeight - rect.bottom - 12;
  const above = rect.top - 12;
  results.style.width = `${rect.width}px`;
  results.style.left = `${rect.left}px`;
  results.style.maxHeight = `${limit}px`;
  const height = results.getBoundingClientRect().height;
  const openBelow = below >= height || below >= above;
  results.style.maxHeight = `${Math.max(0, Math.min(height, openBelow ? below : above))}px`;
  results.style.top = `${openBelow ? rect.bottom + 6 : rect.top - results.offsetHeight - 6}px`;
}

export function installToolbar(dispatch) {
  window.addEventListener('resize',positionSort);
  document.addEventListener('scroll',positionSort,true);
  window.addEventListener('resize', positionTagSearch);
  document.addEventListener('focusin', positionTagSearch);
  document.addEventListener('scroll', event => {
    if (!event.target.matches?.('.tag-search-results')) positionTagSearch();
  }, true);
  document.addEventListener('pointerover', event => {
    const menu=event.target.closest?.('.add-menu');
    if (menu && event.pointerType!=='touch' && !menu.contains(event.relatedTarget)) setAddMenuExpanded(menu,true);
  });
  document.addEventListener('pointerout', event => {
    const menu=event.target.closest?.('.add-menu');
    if (menu && !menu.contains(event.relatedTarget) && !menu.contains(document.activeElement)) setAddMenuExpanded(menu,false);
  });
  document.addEventListener('focusin',event=>{
    const trigger=event.target.closest?.('.add-trigger');
    if(trigger?.matches(':focus-visible')) setAddMenuExpanded(trigger.closest('.add-menu'),true);
  });
  document.addEventListener('focusout',event=>{
    const menu=event.target.closest?.('.add-menu');
    if(menu && !menu.contains(event.relatedTarget)) setAddMenuExpanded(menu,false);
  });
  document.addEventListener('click',event=>{
    const trigger=event.target.closest?.('.add-trigger');
    if(trigger && !trigger.disabled) {
      const menu=trigger.closest('.add-menu');
      setAddMenuExpanded(menu,menu.dataset.expanded!=='true');
    } else {
      const menu=document.querySelector('.add-menu[data-expanded=true]');
      if(menu && !menu.contains(event.target)) setAddMenuExpanded(menu,false);
    }
    if(document.querySelector('#sort-options') && !event.target.closest('.choice-control')) dispatch('sort-close','');
  });
  document.addEventListener('keydown',event=>{
    const search = event.target.closest('.tag-search');
    if (search && !event.isComposing && ['ArrowDown', 'ArrowUp'].includes(event.key)) {
      const options = [...search.querySelectorAll('.tag-search-option')];
      if (!options.length) return;
      const index = options.indexOf(event.target);
      const next = index < 0 ? (event.key === 'ArrowDown' ? 0 : options.length - 1)
        : (index + (event.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length;
      options[next].focus();
      event.preventDefault();
      return;
    }
    if(document.querySelector('[role=dialog]')) return;
    if(event.key==='Escape') {
      if(document.querySelector('#sort-options')) {event.preventDefault();dispatch('sort-close','');return;}
      const menu=document.querySelector('.add-menu[data-expanded=true]');
      if(menu){menu.querySelector('.add-trigger').focus({preventScroll:true});setAddMenuExpanded(menu,false);event.preventDefault();}
    }
    if(event.target.id==='library-sort' && ['ArrowUp','ArrowDown'].includes(event.key)) {
      event.preventDefault();if(!document.querySelector('#sort-options')) dispatch('sort-toggle','');
    } else if(event.target.closest('#sort-options') && ['ArrowUp','ArrowDown','Home','End'].includes(event.key)) {
      event.preventDefault();moveChoiceFocus(document.querySelector('#sort-options'),event.key,event.target);
    }
  });
}
