import { expect, test } from 'bun:test';
import { compileExpress } from './demo.js';

const source = await Bun.file(new URL('./demo.express', import.meta.url)).text();
const output = text => compileExpress(text)[0].createSurface;
const basic = 'https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json';

test('a positional macro call becomes ordinary Basic catalog widgets', () => {
  expect(output(source)).toEqual({surfaceId:'demo', catalogId:basic, components:[
    {id:'root', component:'Button', child:'root_label', action:{event:{name:'greet',context:{recipient:'world'}}}},
    {id:'root_label', component:'Text',text:'Say hello'},
  ]});
});
test('two macros in a column produce independently addressable children', () => {
  const layout = `root = Column(children=[a,b])\na = GreetingButton("A","a","A")\nb = GreetingButton("B","b","B")`;
  const nodes = output(layout).components;
  expect(nodes.map(n => n.id)).toEqual(['root','a','a_label','b','b_label']);
  expect(nodes[0].children).toEqual(['a','b']);
});
test('reject generated ID collision', () => {
  expect(() => output('root = Column(children=[a,a_label])\na = GreetingButton("A","a","A")\na_label = GreetingButton("B","b","B")')).toThrow('collision');
});
test('unknown macros, missing arguments, nonliteral inputs and oversized blocks fail', () => {
  for (const input of ['root = Missing("hello")','root = GreetingButton("hello")', 'root = GreetingButton(123,"greet","world")','a'.repeat(16385)]) expect(() => output(input)).toThrow();
});
test('source-looking text stays a label', () => {
  const label = 'x")\nroot = Missing()';
  const nodes = output(`root = GreetingButton(label=${JSON.stringify(label)},eventName="ok",recipient="demo")`).components;
  expect(nodes).toHaveLength(2);
  expect(nodes[1].text).toBe(label);
});
test('CLI rejects unknown macro without emitting partial wire data', async () => {
  const p = Bun.spawn(['bun','demo.js'], {cwd:import.meta.dir, stdin:new Response('root = Missing()'),stdout:'pipe',stderr:'pipe'});
  expect(await p.exited).not.toBe(0);
  expect(await new Response(p.stdout).text()).toBe('');
});
