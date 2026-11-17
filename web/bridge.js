import {installDetailTagReorder, detailTagClickTarget} from './detail-tag-reorder.js';
import init, * as ui from './pkg/galfriends_ui.js';
import {focusable, syncModalBoundary} from './navigation.js';
import {startGamepad} from './gamepad.js';
import {captureTransition, playTransition} from './transitions.js';
import {captureToolbar, installToolbar, setAddMenuExpanded, syncToolbar} from './toolbar.js';
import {capturePreferences, installPreferences, syncPreferences} from './preferences.js';
import {renderWithTheme, rememberThemeOrigin, applyThemePatch} from './theme-transition.js';
import {installHomeNavigation, syncHomePresentation} from './home.js';
import {renderHomeFrame} from './home-render.js';
import {renderProgressNotice, captureNotices, restoreNotices, syncNoticeTimers} from './progress-notice.js';
import {enhanceChoices, captureChoices, restoreChoices, installChoices} from './choices.js';
import {installLibrarySelection} from './library-selection.js';
import {installLibraryReorder} from './library-reorder.js';

let tagReorder;
let libraryDragging = false;
let deferredLibraryFrame = null;
let pendingReorder = null;
let suppressCardClickUntil = 0;
let wasmReady = false;
let returnFocus = null;
let deferredFocus = null;
let voiceLogFrame = 0;
let voiceLogPatch;
function voiceConsole(id) {
  return document.querySelector(`[data-voice-console="${id}"]`);
}
function followsVoiceLog(output) {
  return output.scrollHeight - output.scrollTop - output.clientHeight <= 4;
}
const identity = node => node && (node.id || node.dataset.action || node.dataset.input) ? {id:node.id,action:node.dataset.action,value:node.dataset.value,input:node.dataset.input} : null;
function locate(key) {
  if (!key) return null;
  if (key.id) return document.getElementById(key.id);
  return [...document.querySelectorAll('[data-action],[data-input]')].find(n => key.input ? n.dataset.input===key.input : n.dataset.action===key.action && n.dataset.value===key.value);
}
window.GalFriendsBridge = {
  report_panic(message) {
    if (window.GalFriendsDiagnostics) void window.GalFriendsDiagnostics.panic(message);
    else console.error(message);
  },
  render_import_progress(id, text) { renderProgressNotice(`import-${id}`, text); },
  render_update_progress(id, text) { renderProgressNotice(`update-${id}`, text); },
  render_emotion_progress(id, text) { renderProgressNotice(`emotion-${id}`, text); },
  render_expression_progress(id, text) { renderProgressNotice(`expression-${id}`, text); },
  render_voice_progress(id, text) {
    renderProgressNotice(`voice-${id}`, text);
    const status = voiceConsole(id)?.querySelector('.voice-console-status');
    if (status) status.textContent = text;
  },
  render_voice_log(id, text) {
    voiceLogPatch = {id, text};
    if (voiceLogFrame) return;
    voiceLogFrame = requestAnimationFrame(() => {
      voiceLogFrame = 0;
      const patch = voiceLogPatch;
      const output = voiceConsole(patch.id)?.querySelector('pre');
      if (!output || output.textContent === patch.text) return;
      const follow = followsVoiceLog(output);
      output.textContent = patch.text;
      if (follow) output.scrollTop = output.scrollHeight;
    });
  },
  render(html) {
    if (libraryDragging) { deferredLibraryFrame = html; return; }
    if(tagReorder?.deferRender(html))return;
    renderWithTheme(html, renderFrame);
  },
  render_theme(html) {
    renderWithTheme(html, patch => {
      const key=identity(document.activeElement)||deferredFocus;
      const preferences=capturePreferences();
      applyThemePatch(patch);
      syncNoticeTimers(ui.expire_notice);
      const target=locate(key);
      deferredFocus=target?.disabled?key:null;
      target?.focus({preventScroll:true});
      syncPreferences(preferences);
    });
  },
  request(id,command,payload) {
    const reorder = command === 'reorder_characters' ? pendingReorder : null;
    if (reorder) reorder.started = true;
    // Defer to release Rust's RefCell borrow before receiving synchronous errors.
    queueMicrotask(async()=>{try{
      const invoke=window.GalFriendsDiagnostics?.invoke || window.__TAURI__?.core?.invoke;
      if(!invoke)throw new Error('Open GalFriends desktop to access the local library.');
      const args=JSON.parse(payload);
      if (command === 'install_app_update' || command === 'install_skill_update') {
        const channel = new window.__TAURI__.core.Channel();
        channel.onmessage = progress => ui.update_progress(id, JSON.stringify(progress));
        args.onProgress = channel;
      }
      if(command==='emotion_recommend') { const channel=new window.__TAURI__.core.Channel(); channel.onmessage=([batch,total])=>ui.emotion_progress(id,batch,total); args.onProgress=channel; }
      if(command==='voice_run' || command==='voice_download') {
        const channel=new window.__TAURI__.core.Channel();
        channel.onmessage=progress=>ui.voice_progress(id,JSON.stringify(progress));
        args.onProgress=channel;
      }
      if(command==='import_character') {
        const channel=new window.__TAURI__.core.Channel();
        channel.onmessage=progress=>ui.import_progress(id,JSON.stringify(progress));
        args.onProgress=channel;
      }
      const result=await invoke(command,args);ui.resolve(id,true,JSON.stringify(result ?? null));
    }catch(error){ui.resolve(id,false,typeof error==='string'?error:JSON.stringify(error instanceof Error?error.message:error));}
    finally {
      reorder?.resolve();
      if (reorder && pendingReorder === reorder) pendingReorder = null;
    }});
  }
};
function renderFrame(html) {
  cancelLibrarySelection();
  const editingTag=!!document.getElementById("detail-tag-input");
  const active=document.activeElement;
  const key=(active?.dataset.action==='voice-cancel' && deferredFocus) || identity(active) || deferredFocus;
  const start=active?.selectionStart, end=active?.selectionEnd;
  const scroll=document.querySelector('.page-scroll')?.scrollTop || 0;
  const previousModal=document.querySelector('#app [role=dialog]');
  const updateScroll=document.getElementById('update-dialog-body')?.scrollTop;
  const previousStartup=document.querySelector('[data-startup]')?.dataset.startup;
  const transition=captureTransition();
  const toolbar=captureToolbar();
  const choices=captureChoices();
  const preferences=capturePreferences();
  const root=document.getElementById('app');
  const notices=captureNotices();
  const player=document.querySelector('.emotion-player');
  const voiceOutput=document.getElementById('voice-console-output');
  const voiceId=voiceOutput?.closest('[data-voice-console]').dataset.voiceConsole;
  const voiceScroll=voiceOutput && !followsVoiceLog(voiceOutput) ? voiceOutput.scrollTop : null;
  if (!renderHomeFrame(root,html)) root.innerHTML=html;
  const nextVoiceOutput=document.getElementById('voice-console-output');
  if (nextVoiceOutput) {
    const sameConsole=nextVoiceOutput.closest('[data-voice-console]').dataset.voiceConsole===voiceId;
    nextVoiceOutput.scrollTop=sameConsole && voiceScroll!==null ? voiceScroll : nextVoiceOutput.scrollHeight;
  }
  enhanceChoices();
  restoreNotices(notices);
  syncNoticeTimers(ui.expire_notice);
  const nextPlayer=document.querySelector(".emotion-player");
  if(player && nextPlayer && player.getAttribute("src")===nextPlayer.getAttribute("src")) nextPlayer.replaceWith(player);
  else if(player) player.pause();
  const shell=document.querySelector('.app-shell');
  document.documentElement.dataset.theme=shell.dataset.theme;
  document.documentElement.dataset.colorTheme=shell.dataset.colorTheme;
  document.documentElement.dataset.fontSize=shell.dataset.fontSize;
  document.documentElement.lang=shell.dataset.language;
  const modal=document.querySelector('#app [role=dialog]');
  const startup=document.querySelector('[data-startup]')?.dataset.startup;
  syncModalBoundary();
  if (modal && modal.getAttribute('aria-labelledby')!==previousModal?.getAttribute('aria-labelledby')) {
    if (!previousModal) returnFocus=key;
    (modal.querySelector('[data-initial-focus]')||modal.querySelector('input,button'))?.focus();
  }
  else if (!modal && previousModal) {
    const target=locate(returnFocus)||document.querySelector('main');
    setAddMenuExpanded(target?.closest('.add-menu'),true);
    deferredFocus=target?.disabled?returnFocus:null;
    if (target?.disabled) document.querySelector('[data-action="voice-cancel"]')?.focus();
    else target?.focus();
    returnFocus=null;
  }
  else {
    const target=locate(key);
    // Saving briefly disables controls. Restore their focus after the response.
    deferredFocus=target?.disabled?key:null;
    target?.focus({preventScroll:true});
    if (target?.setSelectionRange && start!==null) {try{target.setSelectionRange(start,end);}catch{ /* Select controls have no selection range. */ }}
  }
  const updateBody=document.getElementById('update-dialog-body');
  if (updateBody && modal && !modal.contains(document.activeElement)) {
    const fallback = modal.querySelector('[data-initial-focus], [data-action="update-cancel-install"]:not(:disabled)');
    (fallback || updateBody).focus({preventScroll:true});
  }
  if (updateBody && updateScroll!==undefined) updateBody.scrollTop=updateScroll;
  if(startup==='error' && previousStartup!=='error')document.querySelector('[data-action="retry"]')?.focus({preventScroll:true});
  else if(previousStartup && !startup && !modal)document.querySelector('main')?.focus({preventScroll:true});
  const page=document.querySelector('.page-scroll');if(page)page.scrollTop=scroll;
  syncToolbar(toolbar);
  restoreChoices(choices);
  syncPreferences(preferences);
  syncHomePresentation();
  if(!editingTag)document.getElementById("detail-tag-input")?.focus();
  playTransition(transition);
  window.GalFriendsDiagnostics?.viewApplied(startup ? 'boot' : page?.dataset.page, modal ? 'dialog' : 'none');
}
tagReorder=installDetailTagReorder({commit:movement=>ui.dispatch('detail-tag-reorder',JSON.stringify(movement)),render:html=>window.GalFriendsBridge.render(html)});
const cancelLibrarySelection = installLibrarySelection(ui.library_marquee);
installLibraryReorder({
  idsFor: id => wasmReady ? ui.library_drag_ids(id) : '[]',
  start: () => {
    cancelLibrarySelection();
    libraryDragging = true;
    suppressCardClickUntil = Infinity;
    document.getSelection()?.removeAllRanges();
  },
  finish: () => {
    libraryDragging = false;
    suppressCardClickUntil = performance.now() + 400;
    if (deferredLibraryFrame !== null) {
      const html = deferredLibraryFrame;
      deferredLibraryFrame = null;
      window.GalFriendsBridge.render(html);
    }
  },
  // 落下动画与保存均结束后才应用最新 Rust 页面，失败也恢复权威顺序。
  commit: (ids, before) => new Promise(resolve => {
    const completion = {resolve, started: false};
    pendingReorder = completion;
    ui.dispatch('library-reorder', JSON.stringify({ids, before}));
    if (!completion.started) { pendingReorder = null; resolve(); }
  })
});
document.addEventListener('click', event => {
  if (performance.now() < suppressCardClickUntil && event.target.closest('.game-card,.game-grid')) {
    event.preventDefault();
    event.stopImmediatePropagation();
  }
}, true);
installToolbar(ui.dispatch);
installChoices();
installPreferences(ui.dispatch);
installHomeNavigation(ui.dispatch, () => wasmReady);
document.addEventListener('error', event => {
  if (event.target.matches?.('.emotion-player')) ui.dispatch('emotion-play-failed','');
  if (event.target.matches?.('img')) event.target.classList.add('image-unavailable');
}, true);
document.addEventListener('click',event=>{
  const button=detailTagClickTarget(event)||event.target.closest('[data-action]');if(!button||button.disabled)return;
  rememberThemeOrigin(event,button);
  const action=button.dataset.action;ui.dispatch(action,button.dataset.value || '');
  if (['tag-filter-add','tag-filter-remove'].includes(action)) document.getElementById('tag-query')?.focus({preventScroll:true});
  if (action === 'tag-filter-apply' || action === 'tag-clear') document.querySelector('.page-scroll')?.scrollTo(0,0);
  if(['nav','detail','back','folder-open','favorites-open'].includes(action)){document.querySelector('.page-scroll')?.scrollTo(0,0);document.querySelector('main')?.focus({preventScroll:true});}
});
document.addEventListener('input',event=>{if(event.target.id==='detail-tag-input')event.target.closest('.tag-input-wrap').dataset.value=event.target.value;if(!event.isComposing && event.target.dataset.input && event.target.tagName!=='SELECT')ui.input_changed(event.target.dataset.input,event.target.value);});
document.addEventListener('compositionend',event=>{if(event.target.dataset.input)ui.input_changed(event.target.dataset.input,event.target.value);});
document.addEventListener('change',event=>{if(event.target.dataset.input && event.target.tagName==='SELECT')ui.input_changed(event.target.dataset.input,event.target.value);});
document.addEventListener('submit',event=>{if(event.target.dataset.submit){event.preventDefault();ui.dispatch(event.target.dataset.submit,'');}});
document.addEventListener('keydown',event=>{
  rememberThemeOrigin(event,event.target.closest('[data-action]'));
},true);
document.addEventListener('keydown',event=>{
  if(event.target.id==='detail-tag-input' && !event.isComposing && ['Enter','Escape'].includes(event.key)){event.preventDefault();ui.dispatch(event.key==='Enter'?'detail-tag-save':'detail-tag-cancel','');return;}
  const dialog=document.querySelector('[role=dialog]');
  if(event.key==='Escape' && dialog){event.preventDefault();ui.dispatch(dialog.dataset.dismissAction || (dialog.querySelector('[data-action="detail-tag-remove-cancel"]')?'detail-tag-remove-cancel':dialog.querySelector('[data-action="cancel-remove"]')?'cancel-remove':dialog.querySelector('[data-action="library-panel-close"]')?'library-panel-close':'close-search'),'');}
  if(event.key==='Tab' && dialog){const nodes=focusable(dialog);const first=nodes[0],last=nodes.at(-1);if(event.shiftKey&&document.activeElement===first){event.preventDefault();last?.focus();}else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first?.focus();}}
  if(!dialog&&(event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'){event.preventDefault();ui.dispatch('search','');}
});
window.addEventListener('focus',()=>{if(wasmReady && document.querySelector('[data-page=emotions]'))ui.dispatch('emotion-sync','');});
await init();wasmReady=true;
if (window.__TAURI__?.event) {
  try {
    await window.__TAURI__.event.listen('expression-scan-progress', event => ui.expression_scan_progress(JSON.stringify(event.payload)));
    const statuses = await window.__TAURI__.core.invoke('expression_scan_status');
    for (const status of statuses) ui.expression_scan_progress(JSON.stringify(status));
  } catch (error) { console.error('Expression scan subscription failed', error); }
}
ui.start();startGamepad();
window.GalFriendsDiagnostics?.ready();

