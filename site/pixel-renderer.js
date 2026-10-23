// Original pixel artwork, drawn from geometry and tiny bitmap sprites.
// Every frame is a pure function of progress: no random state or advancing clock.
export const clamp = value => Math.min(1, Math.max(0, Number.isFinite(value) ? value : 0));
const smooth = value => { const t = clamp(value); return t * t * (3 - 2 * t); };
const between = (p, a, b) => smooth((p - a) / (b - a));
const mix = (a, b, t) => a + (b - a) * t;
export function chapterAt(progress) { const p = clamp(progress); return p < .23 ? 0 : p < .45 ? 1 : p < .75 ? 2 : 3; }
export const chapters = [0, .27, .53, .87];

const colors = { sky: '#bdc8b8', cloud: '#dbe0ce', distant: '#8eaa9b', outline: '#263e36', dark: '#233b32', wall: '#dbccb0', light: '#f2e7ce', green: '#506d54', leaf: '#829367', shadow: '#9fa590', brick: '#b8ad92', gold: '#e3b97c', blush: '#c88365', teal: '#6d9f96', white: '#edf0d9' };
const letters = {
 A:['01110','10001','10001','11111','10001','10001','10001'],B:['11110','10001','10001','11110','10001','10001','11110'],C:['01111','10000','10000','10000','10000','10000','01111'],D:['11110','10001','10001','10001','10001','10001','11110'],E:['11111','10000','10000','11110','10000','10000','11111'],F:['11111','10000','10000','11110','10000','10000','10000'],G:['01111','10000','10000','10111','10001','10001','01110'],H:['10001','10001','10001','11111','10001','10001','10001'],I:['111','010','010','010','010','010','111'],J:['00111','00010','00010','00010','10010','10010','01100'],K:['10001','10010','10100','11000','10100','10010','10001'],L:['10000','10000','10000','10000','10000','10000','11111'],M:['10001','11011','10101','10101','10001','10001','10001'],N:['10001','11001','10101','10011','10001','10001','10001'],O:['01110','10001','10001','10001','10001','10001','01110'],P:['11110','10001','10001','11110','10000','10000','10000'],Q:['01110','10001','10001','10001','10101','10010','01101'],R:['11110','10001','10001','11110','10100','10010','10001'],S:['01111','10000','10000','01110','00001','00001','11110'],T:['11111','00100','00100','00100','00100','00100','00100'],U:['10001','10001','10001','10001','10001','10001','01110'],V:['10001','10001','10001','10001','10001','01010','00100'],W:['10001','10001','10001','10101','10101','10101','01010'],X:['10001','10001','01010','00100','01010','10001','10001'],Y:['10001','10001','01010','00100','00100','00100','00100'],Z:['11111','00001','00010','00100','01000','10000','11111'],
 '0':['01110','10001','10011','10101','11001','10001','01110'],'1':['010','110','010','010','010','010','111'],'2':['01110','10001','00001','00010','00100','01000','11111'],'3':['11110','00001','00001','01110','00001','00001','11110'],'4':['10010','10010','10010','11111','00010','00010','00010'],'5':['11111','10000','10000','11110','00001','00001','11110'],'6':['01110','10000','10000','11110','10001','10001','01110'],'7':['11111','00001','00010','00100','01000','01000','01000'],'8':['01110','10001','10001','01110','10001','10001','01110'],'9':['01110','10001','10001','01111','00001','00001','01110'],'.':['0','0','0','0','0','1','1'],'/':['00001','00001','00010','00100','01000','10000','10000'],'-':['000','000','000','111','000','000','000'],':':['0','1','1','0','1','1','0']
};

export function renderPixelFrame(ctx, width, height, value) {
  const p = clamp(value), portrait = width < height;
  ctx.save(); ctx.imageSmoothingEnabled = false; ctx.clearRect(0, 0, width, height);
  const rect = (x,y,w,h,color) => { ctx.fillStyle = color; ctx.fillRect(Math.round(x),Math.round(y),Math.round(w),Math.round(h)); };
  const line = (x,y,x2,y2,color,size=1) => { ctx.strokeStyle=color;ctx.lineWidth=size;ctx.beginPath();ctx.moveTo(Math.round(x)+.5,Math.round(y)+.5);ctx.lineTo(Math.round(x2)+.5,Math.round(y2)+.5);ctx.stroke(); };
  const text = (str,x,y,scale=1,color=colors.dark) => {
    let cursor = x;
    for (const ch of str.toUpperCase()) {
      const glyph = letters[ch]; if (!glyph) { cursor += 4*scale;continue; }
      for(let row=0;row<glyph.length;row++)for(let col=0;col<glyph[row].length;col++)if(glyph[row][col]==='1')rect(cursor+col*scale,y+row*scale,scale,scale,color);
      cursor += (glyph[0].length+1)*scale;
    }
  };
  const cup = (x,y,s=1) => { rect(x+1*s,y+3*s,12*s,3*s,colors.dark);rect(x+2*s,y+6*s,10*s,12*s,'#c9b993');rect(x+3*s,y+6*s,3*s,12*s,'#f7efd9');rect(x+3*s,y+10*s,8*s,5*s,colors.green);rect(x+4*s,y+18*s,6*s,2*s,'#c9b993');rect(x+3*s,y+1*s,8*s,2*s,colors.dark); };
  const plant = (x,y,s=1) => { rect(x+5*s,y-22*s,3*s,25*s,colors.green);rect(x-3*s,y-19*s,11*s,6*s,colors.leaf);rect(x+7*s,y-27*s,10*s,7*s,colors.green);rect(x,y-33*s,7*s,12*s,colors.leaf);rect(x-3*s,y,20*s,6*s,colors.blush);rect(x,y+6*s,14*s,18*s,'#a16f55');rect(x+3*s,y+6*s,3*s,16*s,'#bc8b69'); };
  function person(x,y,walk=0,barista=false,hold=false) {
    const stride = Math.round(Math.sin(walk)*4), bob = Math.round(Math.abs(Math.sin(walk))*2);
    ctx.save();ctx.translate(Math.round(x),Math.round(y-bob));
    rect(-10,0,20,3,'#758d78');rect(-7,-15,5,15+stride,colors.dark);rect(3,-15,5,15-stride,colors.dark);rect(-9,0+stride,8,4,'#243830');rect(3,-stride,9,4,'#243830');
    rect(-11,-39,23,27,barista?'#f2e7ce':'#45685c');rect(-9,-36,4,19,barista?'#ddcbab':'#5c8171');rect(-3,-35,12,22,barista?'#b68c62':'#45685c');
    rect(-15,-35,5,18,barista?'#f2e7ce':'#45685c');rect(-15,-18,5,6,'#cf9c7b');
    rect(12,-34,5,13,barista?'#f2e7ce':'#45685c');rect(13,-25,10,5,'#cf9c7b');
    rect(-7,-54,15,16,'#d6a683');rect(6,-49,4,7,'#d6a683');rect(-9,-57,17,8,'#2e4239');rect(-9,-51,4,10,'#2e4239');rect(4,-48,2,2,'#243830');
    if(barista){rect(-11,-59,21,6,colors.light);rect(-5,-63,12,4,colors.light);}else{rect(-10,-40,23,4,'#bcb28b');rect(-8,-36,5,15,'#bcb28b');}
    if(hold)cup(20,-34,.8);else if(!barista){rect(20,-35,9,15,colors.dark);rect(22,-33,5,10,colors.gold);}
    ctx.restore();
  }
  function world() {
    rect(-300,-150,1300,650,colors.sky);
    // Fixed city details: depth comes from camera travel, not random noise.
    for(let i=0;i<9;i++){const x=i*95-120,h=40+(i*29)%70;rect(x,124-h,70,h+90,colors.distant);rect(x+9,112-h,45,12,colors.distant);for(let wy=135-h;wy<180;wy+=19)for(let wx=x+9;wx<x+64;wx+=17)rect(wx,wy,6,8,'#b1c3af');}
    for(let i=0;i<6;i++){const x=i*143-100-p*12;rect(x,21+(i%3)*15,45,5,colors.cloud);rect(x+10,15+(i%3)*15,27,7,colors.cloud);}
    // Tall brick townhouse on the left.
    rect(-35,65,235,213,'#b6b39b');rect(-35,62,235,8,'#7d9280');rect(-35,82,235,3,'#d6d4b9');
    for(let y=95;y<260;y+=13){line(-30,y,200,y,'#a9a98f');for(let x=-30+(y%26?0:15);x<200;x+=30)line(x,y,x,y+12,'#a9a98f');}
    for(const x of [1,68,135])for(const y of [105,178]){rect(x,y,39,52,colors.outline);rect(x+4,y+4,31,44,'#819c8c');rect(x+7,y+7,10,39,'#a6bba5');rect(x+18,y+4,3,44,colors.outline);rect(x,y+54,42,5,'#d6cbb0');}
    // Café, with a visible working interior and pickup counter.
    rect(258,88,325,192,colors.brick);rect(252,82,337,8,colors.outline);rect(265,94,311,11,colors.wall);rect(267,116,300,149,colors.dark);
    rect(276,148,211,110,'#b7ac8c');rect(280,151,203,40,'#d6c09a');
    for(let x=280;x<482;x+=26)line(x,150,x,255,'#a99b7c');for(let y=163;y<251;y+=15)line(280,y,483,y,'#a99b7c');
    rect(294,165,58,24,colors.outline);text('TODAY',300,170,1,colors.light);line(300,181,342,181,colors.gold);
    rect(364,170,40,24,'#657a68');rect(369,174,30,13,'#a7b5a2');rect(370,193,4,7,colors.outline);rect(393,193,4,7,colors.outline);
    for(const x of [318,438]){rect(x,137,2,22,colors.outline);rect(x-9,158,20,5,colors.dark);rect(x-6,163,14,4,colors.gold);}
    person(430,243,0,true,false);
    rect(276,221,211,8,colors.dark);rect(279,229,207,31,'#927959');for(let x=285;x<484;x+=16)rect(x,231,2,27,'#b39a72');
    rect(282,207,17,13,colors.outline);rect(285,209,11,7,colors.teal);
    cup(453,201,1);if(p>.68){for(let i=0;i<3;i++)rect(459+i*3,194-i*5-Math.round(p*80)%4,2,3,colors.light);}
    rect(490,147,67,120,colors.outline);rect(496,152,54,109,'#779686');rect(500,156,17,102,'#9bb09a');rect(543,208,3,10,colors.gold);rect(488,267,73,6,colors.wall);
    rect(269,143,8,124,colors.outline);rect(483,143,8,124,colors.outline);rect(269,261,222,8,colors.outline);
    rect(252,112,337,12,colors.green);for(let x=252;x<589;x+=30)rect(x,112,15,12,colors.light);rect(252,124,337,8,colors.dark);
    rect(326,87,181,25,colors.dark);text('THE USUAL',350,96,2,colors.light);
    // Pavement, tree, café furniture and planters.
    rect(-300,278,1300,100,'#a8ae98');rect(-300,278,1300,6,'#d4d4ba');rect(-300,329,1300,5,'#7c9480');rect(-300,334,1300,100,'#698679');
    for(let x=-200;x<1000;x+=48){line(x,287,x-20,328,'#929f89');line(x,287,x+48,287,'#bec3aa');}
    rect(218,195,9,81,'#6d7054');rect(203,176,40,17,'#536f50');rect(187,145,66,32,'#5e7b55');rect(195,125,50,25,'#7c9363');rect(207,115,28,20,'#8da16e');rect(193,154,16,8,'#809566');
    rect(42,251,7,26,colors.dark);rect(96,251,7,26,colors.dark);rect(37,246,71,7,'#7f7253');rect(37,233,71,6,'#958562');rect(43,231,4,17,colors.dark);rect(99,231,4,17,colors.dark);
    plant(575,259,1.1);plant(273,266,.8);
    rect(290,247,30,38,colors.dark);rect(294,252,22,25,'#687f5e');text('OPEN',296,258,1,colors.light);rect(287,285,6,4,colors.dark);rect(317,285,6,4,colors.dark);
    const walking = p < .23 || p > .75;
    const x = p < .23 ? mix(113,242,between(p,0,.23)) : mix(242,505,between(p,.75,.96));
    person(x,301,walking?p*90:0,false,p>=.94);
    if(p>.94){const a=between(p,.94,.98);ctx.globalAlpha=a;rect(467,187,79,21,colors.light);text('PICKUP READY',473,194,1,colors.dark);ctx.globalAlpha=1;}
  }
  // One camera traverses the same neighbourhood through all four beats.
  ctx.save();
  const focus = portrait ? mix(232,450,between(p,.74,.98)) : mix(310,400,between(p,.74,.98));
  const zoom = (portrait ? 1.04 : width/640) * mix(1,1.18,between(p,.76,.98));
  ctx.translate(width/2,height*.53);ctx.scale(zoom,zoom);ctx.translate(-focus,-170);world();ctx.restore();

  const phoneOpacity = between(p,.18,.235)*(1-between(p,.425,.485));
  if(phoneOpacity>0) {
    ctx.globalAlpha=phoneOpacity*.72;rect(0,0,width,height,'#172d28');ctx.globalAlpha=phoneOpacity;
    ctx.save();ctx.translate(width/2,height/2);const s=Math.min(width/280,height/300)*mix(.88,1,between(p,.18,.25));ctx.scale(s,s);ctx.translate(-90,-132);
    rect(5,6,180,264,'#142a25');rect(0,0,180,260,'#65826d');rect(3,3,174,254,'#203c32');rect(9,10,162,239,colors.light);rect(65,12,50,7,colors.dark);
    text('COSIGNA',23,34,2,colors.dark);line(23,57,156,57,'#bcbfa2');text('YOUR MORNING',23,70,1,colors.green);
    cup(72,88,2);text('THE USUAL CAFE',24,141,1,colors.dark);text('1 OAT FLAT WHITE',24,155,1,colors.green);line(24,173,156,173,'#bcbfa2');text('USD 4.80',24,185,2,colors.dark);
    const approved=p>=.375;rect(20,213,140,24,approved?'#526f48':colors.dark);text(approved?'APPROVED':'APPROVE ONCE',approved?62:43,221,1,colors.light);
    if(p>.31 && p<.412){const tap=between(p,.31,.37)*(1-between(p,.39,.412));const x=mix(198,134,tap),y=mix(277,233,tap);rect(x,y,13,22,'#d6a683');rect(x-6,y+8,26,20,'#d6a683');rect(x-5,y+27,25,22,colors.green);if(p>=.37){ctx.globalAlpha=phoneOpacity*(1-between(p,.375,.41));for(const [dx,dy]of[[-16,0],[19,0],[0,-16],[0,18]])rect(x+dx,y+dy,3,3,colors.gold);}}
    ctx.restore();ctx.globalAlpha=1;
  }
  const processOpacity=between(p,.44,.49)*(1-between(p,.70,.77));
  if(processOpacity>0){
    ctx.globalAlpha=processOpacity*.82;rect(0,0,width,height,'#172d28');ctx.globalAlpha=processOpacity;
    const vertical=portrait, size=vertical?74:100, gap=vertical?24:34, startX=vertical?width/2-size/2:(width-(size*3+gap*2))/2, startY=vertical?height/2-134:height/2-42;
    const steps=[['AUTHORITY','APPROVED',.46],['PROVIDER','CONFIRMED',.57],['RECORD','RECEIPT',.67]];
    steps.forEach(([title,status,threshold],i)=>{
      const x=startX+(vertical?0:i*(size+gap)), y=startY+(vertical?i*(size+gap):0), done=p>=threshold;
      rect(x+4,y+5,size,size,'#152d25');rect(x,y,size,size,done?'#92a77e':'#526e5b');rect(x+2,y+2,size-4,size-4,done?'#e5e6c7':'#29483a');
      text(title,x+8,y+12,1,done?colors.dark:'#b8c7ab');
      if(done){rect(x+size/2-10,y+size/2,6,6,colors.green);rect(x+size/2-4,y+size/2+6,6,6,colors.green);for(let j=0;j<3;j++)rect(x+size/2+2+j*5,y+size/2+1-j*5,5,5,colors.green);}
      else {rect(x+size/2-7,y+size/2-3,14,12,'#829a76');rect(x+size/2-4,y+size/2-9,8,5,'#829a76');}
      text(done?status:'WAITING',x+8,y+size-15,1,done?colors.green:'#b8c7ab');
      if(i<2){const t=clamp((p-(i===0?.49:.60))/.065);for(let j=0;j<gap;j+=6)rect(vertical?x+size/2:x+size+j,vertical?y+size+j:y+size/2,vertical?2:3,vertical?3:2,'#7a967b');rect(vertical?x+size/2-2:x+size+t*gap,vertical?y+size+t*gap:y+size/2-2,6,6,colors.gold);}
    });ctx.globalAlpha=1;
  }
  // Subtle static scanline texture preserves the pixel-art aesthetic at large sizes.
  ctx.globalAlpha=.045;for(let y=0;y<height;y+=3)rect(0,y,width,1,'#102a24');ctx.globalAlpha=1;ctx.restore();
}
