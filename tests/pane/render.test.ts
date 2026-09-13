import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { RenderElement, RenderNode } from 'claude-code';
import { renderToText, softWrap, truncate } from './render';

const box = (props: Record<string, unknown>, ...children: RenderNode[]): RenderElement =>
  ({ type: 'Box', props, children }) as RenderElement;
const text = (s: string, props: Record<string, unknown> = {}): RenderElement =>
  ({ type: 'Text', props, children: [s] }) as RenderElement;

test('column is the default direction', () => {
  assert.deepEqual(renderToText(box({}, text('a'), text('b')), 20), ['a', 'b']);
});

test('row lays children side by side with fixed widths and gap', () => {
  const tree = box({ flexDirection: 'row', gap: 1 }, box({ width: 3 }, text('ab')), box({ width: 4 }, text('cd')));
  assert.deepEqual(renderToText(tree, 20), ['ab  cd']);
});

test('flexGrow takes the leftover width', () => {
  const tree = box(
    { flexDirection: 'row' },
    box({ width: 4 }, text('left')),
    box({ flexGrow: 1 }, text('x'.repeat(30), { wrap: 'truncate' })),
  );
  const rows = renderToText(tree, 20);
  assert.deepEqual(rows, ['left' + 'x'.repeat(15) + '…']);
});

test('justifyContent flex-end right-aligns inside a fixed-width Box', () => {
  const tree = box({ flexDirection: 'row', width: 10, justifyContent: 'flex-end' }, text('123'));
  assert.deepEqual(renderToText(tree, 20), ['       123']);
});

test('paddingX and paddingLeft/Right shrink the content width', () => {
  assert.deepEqual(renderToText(box({ paddingX: 2 }, text('hello world')), 10), ['  hello', '  world']);
  assert.deepEqual(renderToText(box({ paddingLeft: 1, paddingRight: 3 }, text('abcdefgh', { wrap: 'truncate' })), 8), [
    ' abc…',
  ]);
});

test('Text soft-wraps by default and truncates with …', () => {
  assert.deepEqual(renderToText(text('the quick brown fox'), 10), ['the quick', 'brown fox']);
  assert.deepEqual(renderToText(text('the quick brown fox', { wrap: 'truncate' }), 10), ['the quick…']);
  assert.deepEqual(renderToText(text('the quick brown fox', { wrap: 'truncate-start' }), 10), ['…brown fox']);
  assert.deepEqual(renderToText(text('the quick brown fox', { wrap: 'truncate-middle' }), 10), ['the q… fox']);
  assert.deepEqual(softWrap('abcdefghij', 4), ['abcd', 'efgh', 'ij']);
  assert.equal(truncate('ab', 1, 'end'), '…');
});

test('colour and style props are ignored', () => {
  assert.deepEqual(renderToText(text('red', { color: 'red', bold: true, dimColor: true }), 10), ['red']);
});

test('Button renders [hotkey label]', () => {
  const button = { type: 'Button', props: { key: 'tools', label: 'Tools', hotkey: '2' }, press: { plugin: 'p', handle: 1 } };
  assert.deepEqual(renderToText(button as RenderElement, 20), ['[2 Tools]']);
  const plain = { type: 'Button', props: { key: 'Go', label: 'Go' }, press: { plugin: 'p', handle: 2 } };
  assert.deepEqual(renderToText(plain as RenderElement, 20), ['[Go]']);
});

test('Fragment and unknown elements render their children', () => {
  const unknown = { type: 'Mystery', props: {}, children: [text('a'), text('b')] } as unknown as RenderElement;
  assert.deepEqual(renderToText(unknown, 10), ['a', 'b']);
  const fragment = box({ flexDirection: 'column' }, 'plain string', text('c'));
  assert.deepEqual(renderToText(fragment, 12), ['plain string', 'c']);
});

test('no output row is longer than columns', () => {
  const tree = box(
    { flexDirection: 'row', gap: 1 },
    box({ width: 30 }, text('x'.repeat(40))),
    box({ width: 30 }, text('y'.repeat(40), { wrap: 'truncate' })),
    box({ flexGrow: 1 }, text('z'.repeat(40))),
  );
  for (const columns of [20, 50, 80]) {
    for (const row of renderToText(tree, columns)) assert.ok(row.length <= columns, `${columns}: ${row}`);
  }
});

test('rows caps the output', () => {
  assert.deepEqual(renderToText(box({}, text('a'), text('b'), text('c')), 10, 2), ['a', 'b']);
});
