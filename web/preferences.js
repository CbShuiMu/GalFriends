// DOM geometry and keyboard navigation only; Rust owns the selected settings.
const reducedMotion = matchMedia('(prefers-reduced-motion:reduce)');
let resizeObserver;

export function capturePreferences() {
  return {
    model:document.querySelector('.model-picker .choice-trigger[aria-expanded="true"]')?.id,
    modelFocused:!!document.activeElement?.closest('.model-popup'),
    segmentScroll:new Map([...document.querySelectorAll('.segmented-control')].map(group=>[group.id,group.scrollLeft])),
    section: document.querySelector('[data-settings-section]')?.dataset.settingsSection,
    thumbs: new Map([...document.querySelectorAll('.segmented-control')].map(group => {
      const thumb=group.querySelector('.segmented-thumb');
      return [group.id, {transform:getComputedStyle(thumb).transform,width:getComputedStyle(thumb).width}];
    })),
  };
}

function positionThumb(group, previous) {
  const selected=group.querySelector('.segment[aria-pressed="true"]');
  const thumb=group.querySelector('.segmented-thumb');
  if (!selected || !thumb) return;
  const final={transform:`translateX(${selected.offsetLeft}px)`,width:`${selected.offsetWidth}px`};
  Object.assign(thumb.style,final);
  if(previous && !reducedMotion.matches && (previous.transform!==getComputedStyle(thumb).transform || previous.width!==final.width)) {
    thumb.animate([previous,final],{duration:280,easing:'cubic-bezier(.22,.7,.3,1)'});
  }
}

export function syncPreferences(previous) {
  resizeObserver?.disconnect();
  const groups=[...document.querySelectorAll('.segmented-control')];
  for(const group of groups) {
    positionThumb(group,previous.thumbs.get(group.id));
    group.scrollLeft=previous.segmentScroll.get(group.id)||0;
    const selected=group.querySelector('[aria-pressed="true"]');
    if(selected && selected.offsetLeft<group.scrollLeft)group.scrollLeft=selected.offsetLeft;
    else if(selected && selected.offsetLeft+selected.offsetWidth>group.scrollLeft+group.clientWidth)group.scrollLeft=selected.offsetLeft+selected.offsetWidth-group.clientWidth;
  }
  resizeObserver=new ResizeObserver(entries => {for(const {target} of entries) positionThumb(target);});
  groups.forEach(group=>resizeObserver.observe(group));
  positionModels();
  const model=document.querySelector('.model-picker .choice-trigger[aria-expanded="true"]');
  if(model && previous.model!==model.id) document.getElementById('model-query')?.focus({preventScroll:true});
  if(!model && previous.modelFocused) document.getElementById(previous.model)?.focus({preventScroll:true});
  const content=document.querySelector('[data-settings-section]');
  if(content && previous.section && previous.section!==content.dataset.settingsSection) {
    document.querySelector('.page-scroll')?.scrollTo(0,0);
    if(!reducedMotion.matches) content.animate([{opacity:0,transform:'translateY(8px)'},{opacity:1,transform:'translateY(0)'}],{duration:220,easing:'cubic-bezier(.22,1,.36,1)'});
  }
}

export function installPreferences(dispatch) {
  window.addEventListener('resize',positionModels);
  document.addEventListener('scroll',positionModels,true);
  document.addEventListener('click',event=>{
    if(document.querySelector('.model-popup') && !event.target.closest('.model-picker')) dispatch('model-close','');
  });
  document.addEventListener('keydown',event=>{
    const popup=document.querySelector('.model-popup');
    if(event.key==='Escape' && popup) {event.preventDefault();dispatch('model-close','');return;}
    if(event.target.matches('.model-picker .choice-trigger') && ['ArrowDown','ArrowUp'].includes(event.key)) {event.preventDefault();dispatch('model-open',event.target.dataset.value);return;}
    if(!popup?.contains(event.target) || !['ArrowDown','ArrowUp','Home','End'].includes(event.key)) return;
    if(event.target.matches('input') && ['Home','End'].includes(event.key)) return;
    const options=[...popup.querySelectorAll('[role=option]')];
    if(!options.length)return;
    const index=options.indexOf(event.target);
    const next=event.key==='Home'?0:event.key==='End'?options.length-1:index<0?(event.key==='ArrowDown'?0:options.length-1):(index+(event.key==='ArrowDown'?1:-1)+options.length)%options.length;
    event.preventDefault();options[next].focus({preventScroll:true});options[next].scrollIntoView({block:'nearest'});
  });
  document.addEventListener('keydown', event => {
    const segment=event.target.closest('.segment');
    if(!segment || segment.disabled || !['ArrowLeft','ArrowRight','Home','End'].includes(event.key)) return;
    const items=[...segment.closest('.segmented-control').querySelectorAll('.segment:not(:disabled)')];
    const index=items.indexOf(segment);
    const next=event.key==='Home'?0:event.key==='End'?items.length-1:(index+(event.key==='ArrowRight'?1:-1)+items.length)%items.length;
    event.preventDefault();
    items[next].focus({preventScroll:true});
    dispatch(items[next].dataset.action,items[next].dataset.value);
  });
  reducedMotion.addEventListener('change', () => {
    if(reducedMotion.matches) document.querySelectorAll('.segmented-thumb,.preferences-content').forEach(node=>node.getAnimations().forEach(animation=>animation.cancel()));
  });
}

function positionModels() {
  const popup=document.querySelector('.model-popup');
  const trigger=popup?.parentElement.querySelector('.choice-trigger');
  if(!trigger)return;
  const rect=trigger.getBoundingClientRect(),scale=rect.height/trigger.offsetHeight||1;
  const below=(innerHeight-rect.bottom-16)/scale,above=(rect.top-16)/scale;
  const upward=below<200 && above>below;
  const height=Math.max(100,Math.min(280,upward?above:below));
  popup.style.top=upward?'auto':'calc(100% + 7px)';popup.style.bottom=upward?'calc(100% + 7px)':'auto';
  popup.querySelector('.model-options').style.maxHeight=`${height-54}px`;
}
