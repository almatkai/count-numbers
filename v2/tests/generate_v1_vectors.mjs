// Test-only oracle: execute the ORIGINAL updater bodies, without React/JSX or Bun.
// Run from any cwd with Node; regeneration needs the surrounding V1 checkout.
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
const source = readFileSync(new URL('../../src/count-numbers.jsx', import.meta.url), 'utf8');
function blockAfter(marker, after = 0) {
  const start = source.indexOf(marker, after);
  if (start < 0) throw new Error(`Source marker missing: ${marker}`);
  const brace = source.indexOf('{', start + marker.length);
  let depth = 1, end = brace + 1;
  while (depth && end < source.length) {
    if (source[end] === '{') depth++;
    if (source[end] === '}') depth--;
    end++;
  }
  if (depth) throw new Error('Unbalanced source body');
  return source.slice(brace + 1, end - 1);
}
const updateMarker = 'setData((data) =>';
const updater = (marker) => blockAfter(updateMarker, source.indexOf(marker));
const ctx = vm.createContext({ structuredClone, showToast() {}, Toast: {Style:{Failure:'failure'}} });
vm.runInContext('const defaultData = '+source.match(/const defaultData = ([\s\S]*?);\n/)[1]+';'+
  'function getCounter(data) {'+blockAfter('function getCounter(data)')+'}', ctx);
const evaluate = (body, initial, values, reverse=false) => {
  ctx.data=structuredClone(initial); ctx.values=values; ctx.reverse=reverse;
  return vm.runInContext('(function(data){'+body+'})(data)',ctx);
};
const base=vm.runInContext('structuredClone(defaultData)',ctx);
const parses=[];
for (const raw of ['', '0', '-0', '0.9', '-0.9', 'junk', 'NaN', 'Infinity', '  +12cats', '-12.99', '12e3', '0x10tail', '-0Xf', '0b11', '08', '\ufeff-19x', '\u008512', '9007199254740993', '0x20000000000001', '1'+'0'.repeat(308)]) {
 const values={name:'<Counter> & [name]',count:raw,increment:raw,modulo:raw};
 const out=evaluate(updater('const Settings ='),base,values);
 parses.push({input:values,expected:out});
}
const arithmetic=[];
for (const [count,increment,modulo,reverse] of [[0,1,1,false],[0,1,5,true],[-4,1,5,true],[0.1,0.2,1,false],[1e20,1,1,false],[5,-2,-4,false],[4,2,5,false],[-6,1,5,false],[7,0,1,false],[2.5,0.5,2.25,true]]) {
 const data=structuredClone(base); Object.assign(data.counters[0],{count,increment,modulo});
 const expected=evaluate(updater('function incrementCount'),data,null,reverse);
 arithmetic.push({data,reverse,expected});
}
const create=evaluate(updater('const CreateCounter ='),base,{name:'Created',count:'-3.9',increment:'0',modulo:'0x10'});
ctx.data=structuredClone(create);
const deleteSource=blockAfter('function deleteCounter()');
ctx.setData=(fn)=>{ctx.data=fn(ctx.data)};
vm.runInContext('(function(){'+deleteSource+'})()',ctx);
const output={sourceSha256:createHash('sha256').update(source).digest('hex'),parses,arithmetic,create,deleted:ctx.data};
writeFileSync(new URL('./fixtures/v1-vectors.json',import.meta.url),JSON.stringify(output,null,2)+'\n');
