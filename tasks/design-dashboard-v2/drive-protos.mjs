import fs from 'node:fs';
const load = f => { const s = fs.readFileSync(f,'utf8');
  const C = new Function('DCLogic', s.match(/<script data-dc-script[^>]*>([\s\S]*?)<\/script>/)[1]+'\nreturn Component;')(
    class{constructor(p){this.props=p||{}} setState(x){Object.assign(this.state,typeof x==='function'?x(this.state):x)}});
  return new C({}); };
const show = (c, label) => { const v = c.renderVals();
  console.log(`\n--- ${label}  [${v.stateLabel}] ---`);
  console.log(v.rows.map(r=>r.map(g=>g.t).join('').replace(/\s+$/,'')).join('\n')); };

const L = load('Ledger.dc.html');
show(L, 'Ledger · default (sort fill ▼)');
L.setState({ sort:'reading', asc:true }); show(L, 'Ledger · after clicking READING');
L.setState({ filter:'work', sort:'fill', asc:false }); show(L, 'Ledger · filter work');

const K = load('Console.dc.html');
K.setState({ body:'tools' }); show(K, 'Console · after clicking 5');
