// Run in the page by `run.sh` over WebDriver: loads the large competition, then times reporting,
// undoing and filtering until the page shows the change and has drawn the next frame.
const done = arguments[arguments.length - 1];
const frame = () => new Promise(r => requestAnimationFrame(() => r()));
const btn = t => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === t);
const matchRows = () => document.querySelectorAll('tbody')[0]?.rows ?? [];
const standingRows = () => document.querySelectorAll('tbody')[1]?.rows ?? [];
const rowNumbered = n => [...matchRows()].find(r => r.cells[0].textContent.trim() === n);
async function timed(name, action, condition) {
  const start = performance.now();
  action();
  while (!condition()) await frame();
  await frame();
  return [name, Math.round(performance.now() - start)];
}
(async () => {
  const out = [];
  try {
    out.push(await timed('load 1000 entrants, 2500 reported, render',
      () => btn('Load 1000 entrants, 2500 reported').click(),
      () => matchRows().length === 5000 && standingRows().length === 1000));
    for (let i = 1; i <= 2; i++) {
      const button = btn('A wins');
      const number = button.closest('tr').cells[0].textContent.trim();
      out.push(await timed(`report #${number}`, () => button.click(),
        () => rowNumbered(number)?.textContent.includes('won')));
      out.push(await timed('undo (replay of ~2500)', () => btn('Undo').click(),
        () => rowNumbered(number)?.textContent.includes('A wins')));
      const checkbox = document.querySelector('input[type=checkbox]');
      out.push(await timed('filter open only (5000 -> 2500 rows, no engine)', () => checkbox.click(),
        () => matchRows().length === 2500));
      out.push(await timed('show all again (2500 -> 5000 rows, no engine)', () => checkbox.click(),
        () => matchRows().length === 5000));
    }
  } catch (e) { out.push(['JS error', String(e)]); }
  done(out);
})();
