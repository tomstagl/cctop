import fs from 'node:fs';
const src = fs.readFileSync('Main.dc.html','utf8').match(/<script data-dc-script[^>]*>([\s\S]*?)<\/script>/)[1];
class DCLogic { constructor(p){ this.props = p||{}; } setState(s){ Object.assign(this.state,s); } }
const Component = new Function('DCLogic', src + '\nreturn Component;')(DCLogic);

const W = s => Array.from(s).length;
let problems = 0, checked = 0;
for (const alert of ['quiet','nudge','blocked']) {
  for (const cols of [122,80,40]) {
    const c = new Component({ theme:'default-dark', alert });
    c.state = { view:'overview', cols };
    c.screen('overview', cols).forEach((line,i) => {
      const n = line.reduce((a,s)=>a+W(s.t),0);
      checked++;
      if (n > cols) { console.log(`OVERFLOW overview/${alert}/${cols} row ${i}: ${n} cells > ${cols}`); problems++; }
    });
  }
}
for (const view of ['context','cost','tools','events']) {
  for (const cols of [122,80,40]) {
    const c = new Component({ theme:'default-dark', alert:'quiet' });
    c.state = { view, cols };
    c.screen(view, cols).forEach((line,i) => {
      const n = line.reduce((a,s)=>a+W(s.t),0);
      checked++;
      if (n > cols) { console.log(`OVERFLOW ${view}/${cols} row ${i}: ${n} cells > ${cols}  «${line.map(s=>s.t).join('').slice(0,70)}»`); problems++; }
    });
  }
}
console.log(`\n${checked} rows measured, ${problems} overflow${problems===1?'':'s'}`);

// Show the hero screen so the alignment is visible as text
const c = new Component({ theme:'default-dark', alert:'quiet' });
c.state = { view:'overview', cols:122 };
console.log('\n=== overview @122 ===');
console.log(c.screen('overview',122).map(l=>l.map(s=>s.t).join('')).join('\n'));
