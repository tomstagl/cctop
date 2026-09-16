// sRGB <-> OKLab/OKLCH, and a surface-relative sequential ramp per theme accent.
const f=(c)=>c<=0.04045?c/12.92:Math.pow((c+0.055)/1.055,2.4);
const g=(c)=>c<=0.0031308?c*12.92:1.055*Math.pow(c,1/2.4)-0.055;
const hex2rgb=h=>[1,3,5].map(i=>parseInt(h.slice(i,i+2),16)/255);
const rgb2hex=r=>'#'+r.map(v=>Math.round(Math.max(0,Math.min(1,v))*255).toString(16).padStart(2,'0')).join('').toUpperCase();
function rgb2oklab([R,G,B]){const r=f(R),gg=f(G),b=f(B);
 const l=Math.cbrt(0.4122214708*r+0.5363325363*gg+0.0514459929*b);
 const m=Math.cbrt(0.2119034982*r+0.6806995451*gg+0.1073969566*b);
 const s=Math.cbrt(0.0883024619*r+0.2817188376*gg+0.6299787005*b);
 return [0.2104542553*l+0.7936177850*m-0.0040720468*s,1.9779984951*l-2.4285922050*m+0.4505937099*s,0.0259040371*l+0.7827717662*m-0.8086757660*s];}
function oklab2rgb([L,a,b]){const l=(L+0.3963377774*a+0.2158037573*b)**3,m=(L-0.1055613458*a-0.0638541728*b)**3,s=(L-0.0894841775*a-1.2914855480*b)**3;
 return [g(+4.0767416621*l-3.3077115913*m+0.2309699292*s),g(-1.2684380046*l+2.6097574011*m-0.3413193965*s),g(-0.0041960863*l-0.7034186147*m+1.7076147010*s)];}
const lch=h=>{const[L,a,b]=rgb2oklab(hex2rgb(h));return{L,C:Math.hypot(a,b),h:Math.atan2(b,a)};};
const fromLch=(L,C,h)=>rgb2hex(oklab2rgb([L,C*Math.cos(h),C*Math.sin(h)]));
const lum=h=>{const[r,gg,b]=hex2rgb(h).map(f);return 0.2126*r+0.7152*gg+0.0722*b;};
const contrast=(a,b)=>{const[x,y]=[lum(a),lum(b)].sort((p,q)=>q-p);return (x+0.05)/(y+0.05);};

const THEMES={
 'default-dark':{bg:'#0E1318',accent:'#4CC2C2'},'default-light':{bg:'#F1F3F6',accent:'#177F86'},
 'nord':{bg:'#2E3440',accent:'#88C0D0'},'gruvbox':{bg:'#282828',accent:'#83A598'},
 'catppuccin-mocha':{bg:'#1E1E2E',accent:'#89DCEB'},'btop':{bg:'#000000',accent:'#C2A0FF'}};
const dE=(p,q)=>{const A=rgb2oklab(hex2rgb(p)),B=rgb2oklab(hex2rgb(q));return 100*Math.hypot(A[0]-B[0],A[1]-B[1],A[2]-B[2]);};

// Theme::series(n) — the ramp Phase 1 implements in src/theme.rs.
// Hue and chroma from the theme's own accent; lightness carries the order and
// the near endpoint is solved against the theme's background until it clears
// the contrast floor, so a user-authored theme gets a correct ramp for free.
const FLOOR=3.2;
function series(bg, accent, n){
  const a=lch(accent), bgL=lch(bg).L, dark=bgL<0.5, Cnear=a.C*0.28;
  let lo=dark?bgL:0, hi=dark?0.98:bgL;
  for(let i=0;i<40;i++){const m=(lo+hi)/2, ok=contrast(fromLch(m,Cnear,a.h),bg)>=FLOOR; if(dark){ok?hi=m:lo=m}else{ok?lo=m:hi=m}}
  const near=dark?hi:lo;
  const span=n<=3?0.30:0.26;
  const far=dark?Math.min(0.94,Math.max(near+span,a.L+0.14)):Math.max(0.30,Math.min(near-span,a.L-0.10));
  return Array.from({length:n},(_,i)=>{const p=i/(n-1);return fromLch(near+(far-near)*p, a.C*(0.28+0.72*p), a.h);});
}
function check(bg, steps){
  const Ls=steps.map(s=>lch(s).L), dark=lch(bg).L<0.5;
  const mono=Ls.every((v,i)=>i===0||(dark?v>Ls[i-1]:v<Ls[i-1]));
  const cr=steps.map(s=>contrast(s,bg)), ds=steps.slice(1).map((c,i)=>dE(steps[i],c));
  return {mono, minC:Math.min(...cr), ds, worst:Math.min(...ds)};
}

// --- the table in PRD section 5.2: three slices, the overview bar ---
console.log('PRD 5.2 — overview context bar, three slices\n');
console.log('| Theme | fixed | transient | yours | adjacent DeltaE | min contrast |');
console.log('|---|---|---|---|---|---|');
let allMono=true, minAll=99, worstAll=99;
for(const [name,t] of Object.entries(THEMES)){
  const s=series(t.bg,t.accent,3), c=check(t.bg,s);
  allMono=allMono&&c.mono; minAll=Math.min(minAll,c.minC); worstAll=Math.min(worstAll,c.worst);
  console.log(`| ${name} | \`${s[0]}\` | \`${s[1]}\` | \`${s[2]}\` | ${c.ds.map(d=>d.toFixed(1)).join(' / ')} | ${c.minC.toFixed(1)} : 1 |`);
}
console.log(`\nmonotonic lightness, all six: ${allMono?'PASS':'FAIL'}`);
console.log(`min contrast vs surface:      ${minAll.toFixed(2)} : 1  ${minAll>=3?'PASS':'FAIL'}`);
console.log(`worst adjacent DeltaE:        ${worstAll.toFixed(1)}  ${worstAll>=15?'PASS — colour alone is enough':'below 15 on at least one theme; legal only with the alternating glyph stacked_bar() already draws'}`);

// --- panel 1 keeps five, where every slice has its own labelled row ---
console.log('\n\nPanel 1 — five slices (identity comes from the row label, not the hue)\n');
for(const [name,t] of Object.entries(THEMES)){
  const s=series(t.bg,t.accent,5), c=check(t.bg,s);
  console.log(`  ${name.padEnd(18)} ${s.join(' ')}   DeltaE ${c.worst.toFixed(1)}   contrast ${c.minC.toFixed(1)}:1   ${c.mono?'monotonic':'NOT MONOTONIC'}`);
}

// --- what ships today, scored as what it is: a categorical palette ---
console.log('\n\nToday — the status palette used as a series palette (default-dark)\n');
const now=['#4CC2C2','#5FC77E','#E2B04A','#E5605E','#5D6B7D'];
const nd=now.slice(1).map((c,i)=>dE(now[i],c));
console.log(`  accent ok warn crit dim`);
console.log(`  adjacent DeltaE ${nd.map(d=>d.toFixed(1)).join('  ')}`);
console.log(`  worst ${Math.min(...nd).toFixed(1)} — below the categorical floor of 15, and results(ok) sits beside prefix(accent) in the bar.`);
console.log(`  Run the dataviz validator for the full six checks; it reports four FAILs.`);
