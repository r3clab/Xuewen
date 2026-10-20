import { afterEach, describe, expect, it } from 'vitest';
import { menuItems, menuNavKeydown } from './menuNav';

function makeMenu(labels: string[]): HTMLElement {
  const menu = document.createElement('div');
  menu.setAttribute('role', 'menu');
  for (const label of labels) {
    const b = document.createElement('button');
    b.setAttribute('role', 'menuitem');
    b.textContent = label;
    menu.appendChild(b);
  }
  document.body.appendChild(menu);
  return menu;
}

function key(name: string): KeyboardEvent {
  return new KeyboardEvent('keydown', { key: name, cancelable: true });
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('menuItems', () => {
  it('returns the menuitems in order, and [] for null', () => {
    const menu = makeMenu(['a', 'b']);
    expect(menuItems(menu).map((el) => el.textContent)).toEqual(['a', 'b']);
    expect(menuItems(null)).toEqual([]);
  });
});

describe('menuNavKeydown', () => {
  it('ArrowDown/ArrowUp cycle with wrap-around, Home/End jump', () => {
    const menu = makeMenu(['a', 'b', 'c']);
    const [a, b, c] = menuItems(menu);
    a.focus();
    menuNavKeydown(menu, key('ArrowDown'));
    expect(document.activeElement).toBe(b);
    menuNavKeydown(menu, key('ArrowDown'));
    expect(document.activeElement).toBe(c);
    menuNavKeydown(menu, key('ArrowDown')); // wraps to the top
    expect(document.activeElement).toBe(a);
    menuNavKeydown(menu, key('ArrowUp')); // wraps to the bottom
    expect(document.activeElement).toBe(c);
    menuNavKeydown(menu, key('Home'));
    expect(document.activeElement).toBe(a);
    menuNavKeydown(menu, key('End'));
    expect(document.activeElement).toBe(c);
  });

  it('with focus outside the menu, ArrowDown enters at the top and ArrowUp at the bottom', () => {
    const menu = makeMenu(['a', 'b']);
    const [a, b] = menuItems(menu);
    menuNavKeydown(menu, key('ArrowDown'));
    expect(document.activeElement).toBe(a);
    (document.activeElement as HTMLElement).blur();
    menuNavKeydown(menu, key('ArrowUp'));
    expect(document.activeElement).toBe(b);
  });

  it('ignores other keys and empty menus', () => {
    const menu = makeMenu(['a']);
    const e = key('Enter');
    menuNavKeydown(menu, e);
    expect(e.defaultPrevented).toBe(false);
    menuNavKeydown(null, key('ArrowDown')); // no items — no throw
  });
});
