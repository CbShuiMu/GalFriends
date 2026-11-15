import init, * as ui from './pkg/galfriends_ui.js';
import {startGamepad} from './gamepad.js';
import {captureChoices, enhanceChoices, installChoices, restoreChoices} from './choices.js';
import {captureNotices, restoreNotices, syncNoticeTimers} from './progress-notice.js';
let audio = null;
let focusInput = true;
let deferredFocus = null;
let composing = false;
let pendingDisplay = null;
let updatingDisplay = false;
function applyDisplayPreferences(payload) {
  if (composing) {
    pendingDisplay = payload;
    return;
  }
  pendingDisplay = null;
  updatingDisplay = true;
  try { ui.companion_display_changed(JSON.stringify(payload)); }
  finally { updatingDisplay = false; }
}
const nativeWindow = () => window.__TAURI__.window.getCurrentWindow();
window.GalFriendsBridge = {
  render(html) {
    const active = document.activeElement;
    const key = active?.id || active?.dataset.action || deferredFocus;
    const start = active?.selectionStart;
    const end = active?.selectionEnd;
    const notices = captureNotices();
    const choices = updatingDisplay ? captureChoices() : null;
    const scroll = updatingDisplay ? [...document.querySelectorAll('.pet-messages,.pet-profile,#pet-input')]
      .map(node => [node.id ? `#${node.id}` : `.${node.classList[0]}`, node.scrollTop, node.scrollLeft]) : [];
    document.getElementById('app').innerHTML = html;
    const pet = document.querySelector('.pet');
    if (pet) {
      document.documentElement.dataset.theme = pet.dataset.theme;
      document.documentElement.dataset.colorTheme = pet.dataset.colorTheme;
      document.documentElement.dataset.fontSize = pet.dataset.fontSize;
      document.documentElement.lang = pet.lang;
    }
    enhanceChoices();
    if (choices) restoreChoices(choices);
    restoreNotices(notices);
    syncNoticeTimers(ui.companion_expire_notice);
    const input = document.getElementById('pet-input');
    if (key === 'pet-input') focusInput = true;
    const target = document.getElementById(key) || [...document.querySelectorAll('[data-action]')]
      .find(button => button.dataset.action === key) || (focusInput && !input?.disabled ? input : null);
    deferredFocus = target?.disabled ? key : null;
    if (target && !target.disabled) {
      target.focus({preventScroll:true});
      if (start != null && target.id === key) target.setSelectionRange(start,end);
      focusInput = false;
    }
    const log = document.querySelector('.pet-messages');
    if (log && !updatingDisplay) log.scrollTop = log.scrollHeight;
    for (const [selector, top, left] of scroll) {
      const node = document.querySelector(selector);
      if (node) { node.scrollTop = top; node.scrollLeft = left; }
    }
    nativeWindow().isAlwaysOnTop?.().then(pinned=>document.querySelector('[data-native=pin]')?.setAttribute('aria-pressed',String(pinned)));
  },
  request(id, command, payload) {
    queueMicrotask(async () => {
      try { const result = await window.__TAURI__.core.invoke(command, JSON.parse(payload)); ui.companion_resolve(id,true,JSON.stringify(result ?? null)); }
      catch (error) { ui.companion_resolve(id,false,typeof error === 'string' ? error : String(error)); }
    });
  },
  play_audio(data) {
    window.GalFriendsBridge.stop_audio();
    audio = new Audio(data);
    audio.play().catch(() => ui.companion_audio_failed());
  },
  stop_audio() { audio?.pause(); audio = null; }
};
document.addEventListener('input', event => {if (event.target.id==='pet-input' && !event.isComposing) ui.companion_input(event.target.id,event.target.value);});
document.addEventListener('compositionstart', () => { composing = true; });
document.addEventListener('compositionend', event => {
  if (event.target.id==='pet-input') ui.companion_input(event.target.id,event.target.value);
  composing = false;
  // 等待组合输入的最后一次 input，再刷新本地化 DOM；只转交最新通知。
  setTimeout(() => {
    if (pendingDisplay) {
      const payload = pendingDisplay;
      pendingDisplay = null;
      applyDisplayPreferences(payload);
    }
  }, 0);
});
document.addEventListener('change', event => {
  if (event.target.matches('.pet-profile select[data-input]')) {
    ui.companion_input(event.target.dataset.input,event.target.value);
  }
});
document.addEventListener('keydown', event => {
  if (event.target.id==='pet-input' && event.key==='Enter' && !event.shiftKey && !event.isComposing && event.keyCode!==229) {
    event.preventDefault(); focusInput=true; ui.companion_action('send');
  }
});
document.addEventListener('submit',event=>{event.preventDefault();ui.companion_action('send');});
document.addEventListener('pointerdown',event=>{if(event.button===0 && event.target.closest('[data-drag]'))nativeWindow().startDragging().catch(()=>{});});
document.addEventListener('click',async event=>{
  const button=event.target.closest('button');
  if(!button || button.disabled)return;
  if(button.dataset.action){ui.companion_action(button.dataset.action);return;}
  if(button.dataset.native==='close'){window.GalFriendsBridge.stop_audio();await nativeWindow().close();}
  if(button.dataset.native==='pin'){
    const pinned=await nativeWindow().isAlwaysOnTop();
    await nativeWindow().setAlwaysOnTop(!pinned);button.setAttribute('aria-pressed',String(!pinned));
  }
});
installChoices();
await init();
// 先订阅再请求资料，覆盖窗口打开时恰好切换设置的情况。
await window.__TAURI__.event.listen('display-preferences-changed', event => applyDisplayPreferences(event.payload));
ui.companion_start();startGamepad();
