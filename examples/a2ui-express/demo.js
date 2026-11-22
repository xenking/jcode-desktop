import { ExpressParser } from '@a2ui/agent';
import { basicCatalog } from '@a2ui/web_core/catalogs/basic/v1';
import { z } from 'zod';

// Only the producer sees this catalog. The renderer sees Basic components.
const args = z.object({ label: z.string(), eventName: z.string(), recipient: z.string() });
const authoring = basicCatalog.derive({
  catalogId: 'urn:jcode:spike:greeting:1',
  components: ['Column', { name: 'GreetingButton', schema: args }], functions: [],
});

export function compileExpress(source) {
  if (typeof source !== 'string' || Buffer.byteLength(source) > 16384) throw Error('Expected at most 16 KiB of Express');
  const messages = new ExpressParser(authoring, 'demo', '1.0').compile(source);
  const surface = messages[0]?.createSurface;
  if (messages.length !== 1 || surface?.catalogId !== authoring.id || !Array.isArray(surface.components)) throw Error('Expected one complete surface');
  const ids = new Set(surface.components.map(node => node.id));
  if (ids.size !== surface.components.length) throw Error('Duplicate component ID');
  const components = surface.components.flatMap(node => {
    if (node.component === 'Column') return [node];
    if (node.component !== 'GreetingButton') throw Error('Unknown macro');
    const { label, eventName, recipient } = args.parse(node);
    const child = `${node.id}_label`;
    if (ids.has(child)) throw Error(`Macro child ID collision: ${child}`);
    ids.add(child);
    return [
      { id: node.id, component: 'Button', child, action: { event: { name: eventName, context: { recipient } } } },
      { id: child, component: 'Text', text: label },
    ];
  });
  return [{ version: '1.0', createSurface: { ...surface, catalogId: basicCatalog.id, components } }];
}

if (import.meta.main) {
  try {
    const source = await (Bun.argv[2] ? Bun.file(Bun.argv[2]) : Bun.stdin).text();
    console.log(JSON.stringify(compileExpress(source), null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
