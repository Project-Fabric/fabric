import { clamp, chapterAt, chapters, renderPixelFrame } from './pixel-renderer.js';

const canvas=document.querySelector('#pixel-world'), context=canvas.getContext('2d');
const journey=document.querySelector('#journey'), slider=document.querySelector('#timeline');
const buttons=[...document.querySelectorAll('[data-chapter]')], captions=[...document.querySelectorAll('[data-caption]')];
const mode=document.querySelector('#scroll-mode'), reduced=matchMedia('(prefers-reduced-motion: reduce)'), compact=matchMedia('(max-height: 700px)');
let manual=false, target=0, shown=0, frame=0, last=0, currentChapter=-1, visible=true;
document.body.classList.add('ready');
if(!context)document.querySelector('#canvas-fallback').hidden=false;
function render(value) {
  shown=clamp(value);
  if(context)renderPixelFrame(context,canvas.width,canvas.height,shown);
  slider.value=String(Math.round(shown*1000));
  const index=chapterAt(shown);slider.setAttribute('aria-valuetext',`${['Intent','Your approval','Payment process','Pickup'][index]} — ${Math.round(shown*100)} percent`);
  if(index!==currentChapter){currentChapter=index;buttons.forEach((button,i)=>button.setAttribute('aria-pressed',String(i===index)));captions.forEach((caption,i)=>{caption.hidden=i!==index;if(i===index&&!reduced.matches&&caption.animate)caption.animate([{opacity:0,transform:'translateY(6px)'},{opacity:1,transform:'none'}],{duration:320,easing:'ease-out'});});}
}
function tick(time){frame=0;if(document.hidden||!visible){last=0;return;}const dt=last?Math.min(50,time-last):16.67;last=time;const next=reduced.matches||manual?target:shown+(target-shown)*(1-Math.exp(-dt/65));render(Math.abs(target-next)<.0002?target:next);if(Math.abs(target-shown)>.0002)frame=requestAnimationFrame(tick);else last=0;}
function schedule(){if(!frame&&!document.hidden&&visible)frame=requestAnimationFrame(tick);}
function scrollUpdate(){if(manual||reduced.matches||compact.matches)return;const rect=journey.getBoundingClientRect();target=clamp(-rect.top/Math.max(1,journey.offsetHeight-innerHeight));schedule();}
function setMode(){const enabled=!manual&&!reduced.matches&&!compact.matches;journey.classList.toggle('scroll-driven',enabled);mode.setAttribute('aria-pressed',String(!enabled));mode.textContent=enabled?'Use manual controls':reduced.matches||compact.matches?'Manual controls enabled':'Resume scroll story';mode.disabled=reduced.matches||compact.matches;scrollUpdate();}
function select(value){target=clamp(value);render(target);if(!manual&&!reduced.matches&&!compact.matches){const top=scrollY+journey.getBoundingClientRect().top;window.scrollTo({top:top+target*Math.max(0,journey.offsetHeight-innerHeight),behavior:'instant'});}}
buttons.forEach((button,index)=>button.addEventListener('click',()=>select(chapters[index])));
slider.addEventListener('input',()=>select(Number(slider.value)/1000));
mode.addEventListener('click',()=>{const top=journey.getBoundingClientRect().top;manual=!manual;setMode();if(!manual)select(shown);else if(top<0)window.scrollTo({top:scrollY+journey.getBoundingClientRect().top,behavior:'instant'});});
function resize(){const portrait=canvas.getBoundingClientRect().width<600;canvas.width=portrait?360:640;canvas.height=portrait?420:360;render(shown);scrollUpdate();}
window.addEventListener('scroll',scrollUpdate,{passive:true});window.addEventListener('resize',resize);
reduced.addEventListener('change',()=>{cancelAnimationFrame(frame);frame=0;last=0;setMode();render(target);});compact.addEventListener('change',setMode);
document.addEventListener('visibilitychange',()=>{if(document.hidden){cancelAnimationFrame(frame);frame=0;last=0;}else{scrollUpdate();schedule();}});
if('IntersectionObserver' in window)new IntersectionObserver(entries=>{visible=entries[0].isIntersecting;if(visible){scrollUpdate();schedule();}else{cancelAnimationFrame(frame);frame=0;last=0;}},{rootMargin:'100px'}).observe(journey);
if('ResizeObserver' in window)new ResizeObserver(resize).observe(canvas);
setMode();resize();render(target);
