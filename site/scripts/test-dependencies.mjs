import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import test from 'node:test';

const require = createRequire(import.meta.url);
const braces = require('braces');
const nested = (open, close, depth) => open.repeat(depth) + 'a,b' + close.repeat(depth);

for (const [name, operation] of Object.entries({
  default: braces,
  compile: braces.compile,
  expand: braces.expand,
  stringify: braces.stringify,
  parse: braces.parse,
})) {
  test(`${name} rejects excessive nesting before recursive traversal`, () => {
    for (const [open, close] of [['{', '}'], ['(', ')'], ['{(', ')}']]) {
      assert.throws(() => operation(nested(open, close, open.length === 1 ? 4000 : 2000)), {
        name: 'SyntaxError',
        message: 'Input nesting exceeds maximum depth (100)',
      });
    }
  });
}

test('ordinary brace patterns retain compile and expansion behavior', () => {
  assert.equal(braces.compile('src/{app,lib}/*.{js,ts}'), 'src/(app|lib)/*.(js|ts)');
  assert.deepEqual(braces.expand('a/{b,c}/{1..3}'), [
    'a/b/1', 'a/b/2', 'a/b/3', 'a/c/1', 'a/c/2', 'a/c/3',
  ]);
  const pattern = nested('{', '}', 100);
  assert.equal(braces.stringify(pattern), pattern);
  assert.doesNotThrow(() => braces.compile(pattern));
  assert.doesNotThrow(() => braces.expand(pattern));
});

test('escaped, quoted and character-class braces remain literal', () => {
  for (const pattern of ['\\{'.repeat(200), '"' + '{'.repeat(200) + '"', '[' + '{'.repeat(200) + ']']) {
    assert.doesNotThrow(() => braces.parse(pattern));
  }
});

for (const operation of ['compile', 'expand', 'stringify']) {
  test(`${operation} bounds directly supplied ASTs`, () => {
    let ast = { type: 'text', value: 'a' };
    for (let i = 0; i < 4000; i++) {
      const parent = { type: 'root', nodes: [ast] };
      ast.parent = parent;
      ast = parent;
    }
    assert.throws(() => braces[operation](ast), {
      name: 'SyntaxError',
      message: 'Input nesting exceeds maximum depth (100)',
    });
  });
}

test('micromatch resolves the patched package through the override', () => {
  const fromMicromatch = createRequire(require.resolve('micromatch'));
  assert.equal(fromMicromatch.resolve('braces'), require.resolve('braces'));
  assert.equal(fromMicromatch('braces/package.json').version, '3.0.3-safeselect.1');
  assert.deepEqual(require('micromatch')(['a.js', 'b.ts', 'c.css'], '*.{js,ts}'), ['a.js', 'b.ts']);
});
