const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const {test} = require('node:test');

const source = fs.readFileSync(path.join(__dirname, '../web/theme-transition.js'), 'utf8');

function fixture({reduce = false, supported = true} = {}) {
  const callbacks = [], transitions = [], rendered = [];
  const dataset = {theme: 'light', colorTheme: 'pink'};
  let motionChange;
  const motion = {matches: reduce, addEventListener(_, callback) { motionChange = callback; }};
  const document = {
    documentElement: {dataset},
    querySelector: () => null,
    startViewTransition: supported ? callback => {
      callbacks.push(callback);
      const transition = {ready: Promise.resolve(), finished: Promise.resolve(),
        skipTransition() { this.skipped = true; }};
      transitions.push(transition);
      return transition;
    } : undefined,
  };
  const context = vm.createContext({document, matchMedia: () => motion, clearTimeout, setTimeout});
  vm.runInContext(source.replaceAll('export function', 'function'), context);
  return {
    callbacks, transitions, rendered, dataset,
    render(theme, color = 'pink') {
      context.renderWithTheme(`<div data-theme="${theme}" data-color-theme="${color}"></div>`, () => {
        dataset.theme = theme;
        dataset.colorTheme = color;
        rendered.push(`${theme}/${color}`);
      });
    },
    reduce() { motion.matches = true; motionChange(); },
  };
}

test('late snapshot callbacks cannot overwrite newer navigation or theme', () => {
  const app = fixture();
  app.render('dark');
  app.render('light');
  app.callbacks[0]();
  assert.equal(app.transitions[0].skipped, true);
  assert.deepEqual(app.rendered, ['light/pink']);
  app.render('dark');
  app.render('dark', 'white');
  app.callbacks[2]();
  app.callbacks[1]();
  assert.deepEqual(app.rendered, ['light/pink', 'dark/white']);
});

test('unchanged theme and reduced motion commit immediately without a snapshot', () => {
  const app = fixture({reduce: true});
  app.render('light');
  app.render('dark');
  assert.equal(app.transitions.length, 0);
  assert.deepEqual(app.rendered, ['light/pink', 'dark/pink']);
});

test('reducing motion during capture skips animation but still commits the saved theme', () => {
  const app = fixture();
  app.render('dark');
  app.reduce();
  app.callbacks[0]();
  assert.equal(app.transitions[0].skipped, true);
  assert.equal(app.dataset.theme, 'dark');
});

test('older WebViews apply saved theme even when there is no shell to clone', () => {
  const app = fixture({supported: false});
  app.render('dark');
  assert.deepEqual(app.rendered, ['dark/pink']);
});
