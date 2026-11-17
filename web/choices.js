// 选择值和可用选项仍由 Rust 渲染；这里只增强原生 select 的外观、展开与焦点。
const controlSelector = '.choice-control[data-choice-input]';
const openControl = () => document.querySelector(`${controlSelector}:has(.choice-popup)`);

export function positionChoice(trigger, popup) {
  if (!trigger || !popup) return;
  const rect = trigger.getBoundingClientRect();
  const available = Math.max(0, innerWidth - 16);
  popup.style.minWidth = `${Math.min(rect.width, available)}px`;
  popup.style.maxWidth = `${Math.min(available, Math.max(rect.width, 360))}px`;
  popup.style.width = 'max-content';
  popup.style.maxHeight = `${Math.max(0, Math.min(340, innerHeight - 24))}px`;
  const box = popup.getBoundingClientRect();
  const below = innerHeight - rect.bottom - 12, above = rect.top - 12;
  const bottomSide = below >= box.height || below >= above;
  popup.style.maxHeight = `${Math.max(0, Math.min(box.height, bottomSide ? below : above))}px`;
  popup.style.left = `${Math.max(8, Math.min(rect.left, innerWidth - box.width - 8))}px`;
  popup.style.top = `${bottomSide ? rect.bottom + 6 : Math.max(8, rect.top - popup.getBoundingClientRect().height - 6)}px`;
}

export function moveChoiceFocus(popup, key, active) {
  const options = [...popup.querySelectorAll('[role=option]:not(:disabled)')];
  const index = options.indexOf(active);
  const next = key === 'Home' ? 0 : key === 'End' ? options.length - 1
    : (index + (key === 'ArrowDown' ? 1 : -1) + options.length) % options.length;
  options[next]?.focus({preventScroll:true});
  options[next]?.scrollIntoView({block:'nearest'});
}

function symbol(path) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  for (const [name, value] of Object.entries({class:'icon',viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':'1.8','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'})) {
    svg.setAttribute(name, value);
  }
  const line = document.createElementNS(svg.namespaceURI, 'path');
  line.setAttribute('d', path);
  svg.append(line);
  return svg;
}

export function enhanceChoices() {
  for (const select of document.querySelectorAll('.voice-workbench select[data-input], .emotion-toolbar select[data-input], .pet-profile select[data-input]')) {
    if (select.closest(controlSelector)) continue;
    // 无占位选项时，浏览器会自动选中第一项。以 Rust 输出的选中状态为准。
    if (select.hasAttribute('data-choice-allow-empty') && ![...select.options].some(option => option.defaultSelected)) {
      select.selectedIndex = -1;
    }
    const labels = [...select.labels];
    const label = select.getAttribute('aria-label') || labels.map(node =>
      (node.querySelector(':scope > span') || node).textContent.trim()).join(' ');
    const control = document.createElement('div');
    control.className = 'choice-control';
    control.dataset.choiceInput = select.dataset.input;
    const trigger = document.createElement('button');
    trigger.type = 'button';
    trigger.id = select.id || `${select.dataset.input}-choice`;
    select.removeAttribute('id');
    trigger.className = 'choice-trigger';
    trigger.disabled = select.disabled || !select.options.length;
    for (const [name, value] of Object.entries({role:'combobox','aria-label':label,'aria-expanded':'false','aria-haspopup':'listbox','aria-controls':`${trigger.id}-options`})) {
      trigger.setAttribute(name, value);
    }
    const copy = document.createElement('span');
    copy.className = 'choice-copy';
    const text = document.createElement('span');
    text.textContent = select.selectedOptions[0]?.label || '';
    trigger.title = text.textContent || label;
    copy.append(text);
    trigger.append(copy, symbol('m7 10 5 5 5-5'));
    labels.forEach(node => node.htmlFor = trigger.id);
    select.hidden = true;
    select.tabIndex = -1;
    select.setAttribute('aria-hidden', 'true');
    select.replaceWith(control);
    control.append(select, trigger);
  }
}

let search = '', searchAt = 0;
function closeChoice(control, restoreFocus = false) {
  if (!control) return;
  const trigger = control.querySelector('.choice-trigger');
  if (restoreFocus) trigger.focus({preventScroll:true});
  trigger.classList.remove('open');
  trigger.setAttribute('aria-expanded', 'false');
  control.querySelector('.choice-popup')?.remove();
  search = '';
}

function openChoice(control, focus = true) {
  const trigger = control.querySelector('.choice-trigger');
  if (trigger.disabled || control.querySelector('.choice-popup')) return;
  closeChoice(openControl());
  const select = control.querySelector('select');
  const popup = document.createElement('div');
  popup.id = trigger.getAttribute('aria-controls');
  popup.className = 'choice-popup';
  popup.setAttribute('role', 'listbox');
  popup.setAttribute('aria-label', trigger.getAttribute('aria-label'));
  for (const option of select.options) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = `choice-option${option.selected ? ' selected' : ''}`;
    button.tabIndex = -1;
    button.dataset.choiceValue = option.value;
    button.setAttribute('role', 'option');
    button.setAttribute('aria-selected', String(option.selected));
    button.disabled = option.disabled;
    const text = document.createElement('span');
    text.className = 'choice-label';
    text.textContent = option.label;
    button.title = option.label;
    const check = document.createElement('span');
    check.className = 'choice-check';
    if (option.selected) check.append(symbol('m5 12 4 4L19 6'));
    button.append(text, check);
    popup.append(button);
  }
  control.append(popup);
  trigger.classList.add('open');
  trigger.setAttribute('aria-expanded', 'true');
  positionChoice(trigger, popup);
  if (focus) {
    const selected = popup.querySelector('[aria-selected=true]:not(:disabled)') || popup.querySelector('button:not(:disabled)');
    selected?.focus({preventScroll:true});
    selected?.scrollIntoView({block:'nearest'});
  }
}

export function captureChoices() {
  const control = openControl();
  return control && {
    input: control.dataset.choiceInput,
    focused: document.activeElement?.closest('.choice-option')?.dataset.choiceValue,
    scroll: control.querySelector('.choice-popup').scrollTop,
  };
}

export function restoreChoices(previous) {
  if (!previous || document.querySelector('[role=dialog]')) return;
  const control = [...document.querySelectorAll(controlSelector)].find(node => node.dataset.choiceInput === previous.input);
  if (!control || control.closest('[inert]')) return;
  openChoice(control, false);
  const popup = control.querySelector('.choice-popup');
  if (!popup) return;
  popup.scrollTop = previous.scroll;
  if (previous.focused !== undefined) {
    const option = [...popup.querySelectorAll('.choice-option')].find(node => node.dataset.choiceValue === previous.focused && !node.disabled);
    (option || control.querySelector('.choice-trigger')).focus({preventScroll:true});
  }
}

export function installChoices() {
  const reposition = () => {
    const control = openControl();
    if (control) positionChoice(control.querySelector('.choice-trigger'), control.querySelector('.choice-popup'));
  };
  window.addEventListener('resize', reposition);
  document.addEventListener('scroll', reposition, true);
  document.addEventListener('pointerdown', event => {
    const control = openControl();
    if (control && !control.contains(event.target)) closeChoice(control);
  });
  document.addEventListener('focusin', event => {
    const control = openControl();
    if (control && !control.contains(event.target)) closeChoice(control);
  });
  document.addEventListener('click', event => {
    const control = event.target.closest?.(controlSelector);
    if (!control) return;
    const trigger = event.target.closest('.choice-trigger');
    if (trigger && !trigger.disabled) {
      control.querySelector('.choice-popup') ? closeChoice(control, true) : openChoice(control);
      return;
    }
    const option = event.target.closest('.choice-option');
    const select = control.querySelector('select');
    if (!option || option.disabled || select.disabled) return;
    closeChoice(control, true);
    if (select.value !== option.dataset.choiceValue) {
      select.value = option.dataset.choiceValue;
      select.dispatchEvent(new Event('change', {bubbles:true}));
    }
  });
  document.addEventListener('keydown', event => {
    const control = event.target.closest?.(controlSelector);
    if (!control || control.querySelector('.choice-trigger').disabled) return;
    const popup = control.querySelector('.choice-popup');
    if (event.key === 'Escape' && popup) {
      event.preventDefault();
      closeChoice(control, true);
    } else if (event.key === 'Tab' && popup) {
      closeChoice(control, true);
    } else if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      if (!popup) openChoice(control);
      if (popup || ['Home', 'End'].includes(event.key)) {
        moveChoiceFocus(control.querySelector('.choice-popup'), event.key, document.activeElement);
      }
    } else if (event.key.length === 1 && event.key !== ' ' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.isComposing) {
      event.preventDefault();
      if (!popup) openChoice(control);
      const now = performance.now();
      search = (now - searchAt < 700 ? search : '') + event.key.toLocaleLowerCase();
      searchAt = now;
      const prefix = [...search].every(char => char === search[0]) ? search[0] : search;
      const options = [...control.querySelectorAll('.choice-option:not(:disabled)')];
      const start = options.indexOf(document.activeElement) + (prefix.length === 1 ? 1 : 0);
      const ordered = [...options.slice(start), ...options.slice(0, start)];
      const match = ordered.find(node => node.title.toLocaleLowerCase().startsWith(prefix));
      match?.focus({preventScroll:true});
      match?.scrollIntoView({block:'nearest'});
    }
  });
}
