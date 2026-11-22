// Usage: bun check.js <pinned a2ui checkout> <agent|renderer> <payload.json|->
import Ajv from 'ajv/dist/2020.js';
import formats from 'ajv-formats';
import { join } from 'node:path';

const [checkout, direction, filename] = Bun.argv.slice(2);
const envelopes = { agent: 'agent_to_renderer', renderer: 'renderer_to_agent' };
if (!checkout || !Object.hasOwn(envelopes, direction) || !filename) throw Error('Expected checkout, agent|renderer, and JSON file or -');
const ajv = new Ajv({ strict: false });
formats(ajv);
const schema = name => Bun.file(join(checkout, `specification/v1_0/json/${name}.json`)).json();
const common = await schema('common_types');
const catalog = await Bun.file(join(checkout, 'catalogs/basic/v1/catalog.json')).json();
// Same catalog alias as the upstream v1_0 test runner; no network reference resolution.
catalog.$id = new URL('catalog.json', common.$id).href;
ajv.addSchema(common).addSchema(catalog);
const validate = ajv.compile(await schema(envelopes[direction]));
const input = await (filename === '-' ? Bun.stdin : Bun.file(filename)).json();
const messages = Array.isArray(input) ? input : [input];
if (!messages.length) throw Error('No messages');
for (const [index, message] of messages.entries()) {
  if (!validate(message)) throw Error(`Message ${index}: ${ajv.errorsText(validate.errors)}`);
  for (const node of (message.createSurface ?? message.updateComponents)?.components ?? []) {
    if (!Object.hasOwn(catalog.components, node.component)) throw Error(`Not a Basic component: ${node.component}`);
    const checkNode = ajv.getSchema(`${catalog.$id}#/components/${node.component}`);
    if (!checkNode(node)) throw Error(`${node.id}: ${ajv.errorsText(checkNode.errors)}`);
  }
}
console.error(`OK: ${messages.length} ${direction} message(s), A2UI 1.0 Basic`);
