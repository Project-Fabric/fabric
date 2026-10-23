import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { JSDOM } from 'jsdom';
import { clamp, chapterAt, chapters, renderPixelFrame } from '../../site/pixel-renderer.js';

function renderHash(width,height,progress){
  const hash=createHash('sha256');let depth=0;
  const context=new Proxy({}, {get(_target,key){return (...args)=>{for(const value of args)if(typeof value==='number')assert.ok(Number.isFinite(value),`${key} coordinates must be finite`);if(key==='save')depth++;if(key==='restore')depth--;hash.update(JSON.stringify([key,...args]));};},set(_target,key,value){hash.update(JSON.stringify([key,value]));return true;}});
  renderPixelFrame(context,width,height,progress);assert.equal(depth,0,'Drawing state must be restored');return hash.digest('hex');
}

test('pixel frames scrub deterministically in both directions on desktop and portrait',()=>{
  for(const [w,h]of[[640,360],[360,420]]){
    const times=[0,.1,.22,.34,.375,.46,.58,.67,.74,.87,.96,1];
    const hashes=times.map(time=>renderHash(w,h,time));assert.equal(new Set(hashes).size,times.length);
    for(let i=times.length-1;i>=0;i--)assert.equal(renderHash(w,h,times[i]),hashes[i]);
    assert.equal(renderHash(w,h,-4),hashes[0]);assert.equal(renderHash(w,h,NaN),hashes[0]);assert.equal(renderHash(w,h,9),hashes.at(-1));
  }
});

function preview({reduced=false,compact=false}={}){
  const dom=new JSDOM(readFileSync(new URL('../../site/pixel.html',import.meta.url),'utf8'),{runScripts:'outside-only',url:'http://127.0.0.1:3091/concept',pretendToBeVisual:true});
  const win=dom.window,doc=win.document,frames=new Map(),draws=[];let id=0,time=0,top=0,scroll=0;
  const media=new Map();win.matchMedia=query=>{if(!media.has(query))media.set(query,{matches:query.includes('reduced-motion')?reduced:compact,addEventListener(_name,listener){this.listener=listener;}});return media.get(query);};
  win.requestAnimationFrame=fn=>{frames.set(++id,fn);return id;};win.cancelAnimationFrame=id=>frames.delete(id);
  Object.defineProperty(win,'innerHeight',{value:900});Object.defineProperty(win,'scrollY',{get:()=>scroll});
  const journey=doc.querySelector('#journey');Object.defineProperty(journey,'offsetHeight',{get:()=>journey.classList.contains('scroll-driven')?4500:900});
  journey.getBoundingClientRect=()=>({top});
  win.scrollTo=options=>{scroll=options.top;top=-scroll;win.dispatchEvent(new win.Event('scroll'));};
  win.HTMLCanvasElement.prototype.getContext=()=>({});win.HTMLCanvasElement.prototype.getBoundingClientRect=()=>({width:800,height:450});
  win.pixel={clamp,chapterAt,chapters,renderPixelFrame:(_context,_w,_h,p)=>draws.push(p)};
  win.eval(readFileSync(new URL('../../site/pixel.js',import.meta.url),'utf8').replace(/^import[^\n]+\n/,'const {clamp,chapterAt,chapters,renderPixelFrame}=window.pixel;\n'));
  function settle(){for(let i=0;frames.size&&i<100;i++){for(const [id,fn]of[...frames]){frames.delete(id);fn(time+=16.67);}}assert.equal(frames.size,0,'Renderer stops scheduling when settled');}
  return {dom,win,doc,journey,draws,settle,scroll(p){win.scrollTo({top:p*3600});settle();}};
}

test('scroll reaches each narrative beat and reverses without executing a request',()=>{
  const p=preview();try{
    for(const [value,index]of[[0,0],[.3,1],[.6,2],[.9,3],[.3,1],[0,0]]){p.scroll(value);assert.equal(p.doc.querySelector(`[data-chapter="${index}"]`).getAttribute('aria-pressed'),'true');assert.equal(p.doc.querySelectorAll('[data-caption]:not([hidden])').length,1);assert.ok(Math.abs(p.draws.at(-1)-value)<.001);}
  }finally{p.dom.window.close();}
});

test('chapter buttons and timeline work with reduced motion and compact screens',()=>{
  for(const options of[{reduced:true},{compact:true}]){const p=preview(options);try{
    assert.equal(p.journey.classList.contains('scroll-driven'),false);assert.equal(p.doc.querySelector('#scroll-mode').disabled,true);
    p.doc.querySelector('[data-chapter="3"]').click();assert.equal(p.doc.querySelector('[data-chapter="3"]').getAttribute('aria-pressed'),'true');
    const range=p.doc.querySelector('#timeline');range.value='320';range.dispatchEvent(new p.win.Event('input'));assert.match(range.getAttribute('aria-valuetext'),/Your approval/);
    p.scroll(.9);assert.equal(p.draws.at(-1),.32);
  }finally{p.dom.window.close();}}
});

test('manual mode freezes the scroll-driven sequence and can resume it',()=>{
  const p=preview();try{
    p.scroll(.3);const button=p.doc.querySelector('#scroll-mode');button.click();assert.equal(button.getAttribute('aria-pressed'),'true');assert.equal(p.journey.classList.contains('scroll-driven'),false);
    p.doc.querySelector('[data-chapter="2"]').click();p.scroll(.9);assert.equal(p.draws.at(-1),chapters[2]);
    button.click();p.settle();assert.equal(p.journey.classList.contains('scroll-driven'),true);assert.equal(button.getAttribute('aria-pressed'),'false');
  }finally{p.dom.window.close();}
});
