import { technologyIconPath } from './technology-icon-path';

interface Ingredient {
  id: string;
  label: string;
  icon: string | null;
  glyph: string;
  color: string;
  darkColor: string;
  lines: string[];
}

const stages = [
  'Reading your stack',
  'Picking the right ingredients',
  'Finding where everything belongs',
  'Writing a clean devenv.nix',
  'Checking every declaration',
];

const paths = [
  ['-13rem', '-4.8rem'],
  ['12.5rem', '-3.5rem'],
  ['-12rem', '4.6rem'],
  ['13.5rem', '4.2rem'],
  ['0rem', '-7.4rem'],
];

function createIcon(ingredient: Ingredient) {
  const icon = document.createElement('span');
  icon.className = 'landing-ai-ingredient-icon';
  if (!ingredient.icon) {
    icon.textContent = ingredient.glyph;
    return icon;
  }

  const image = document.createElement('img');
  image.src = technologyIconPath(ingredient.icon, displayColor(ingredient));
  image.alt = '';
  image.width = 22;
  image.height = 22;
  image.addEventListener('error', () => {
    icon.textContent = ingredient.glyph;
  }, { once: true });
  icon.appendChild(image);
  return icon;
}

function displayColor(ingredient: Ingredient) {
  return document.documentElement.dataset.theme === 'dark' ? ingredient.darkColor : ingredient.color;
}

function declarationFor(ingredient: Ingredient) {
  const declaration = ingredient.lines.find((line) => line.trim() && !line.trim().startsWith('#'));
  return (declaration || ingredient.id).trim();
}

function requiredElement<T extends Element>(element: T | null) {
  if (!element) throw new Error('Incomplete AI composition loader');
  return element;
}

export function createAiCompositionLoader(loader: HTMLElement, catalog: Ingredient[]) {
  const inspector = requiredElement(loader.closest<HTMLElement>('[data-file-dropzone]'));
  const stage = requiredElement(loader.querySelector<HTMLElement>('[data-ai-loader-stage]'));
  const fileLines = requiredElement(loader.querySelector<HTMLElement>('[data-ai-file-lines]'));
  const ingredients = requiredElement(loader.querySelector<HTMLElement>('[data-ai-ingredients]'));

  let stageTimer: ReturnType<typeof setInterval> | null = null;
  let closeTimer: ReturnType<typeof setTimeout> | null = null;

  function clearTimers() {
    if (stageTimer) clearInterval(stageTimer);
    if (closeTimer) clearTimeout(closeTimer);
    stageTimer = null;
    closeTimer = null;
  }

  function render(selectedIds: string[]) {
    const selected = selectedIds
      .map((id) => catalog.find((ingredient) => ingredient.id === id))
      .filter((ingredient): ingredient is Ingredient => Boolean(ingredient));
    const fallbacks = ['languages.nix', 'packages.git', 'services.postgres']
      .map((id) => catalog.find((ingredient) => ingredient.id === id))
      .filter((ingredient): ingredient is Ingredient => Boolean(ingredient));
    const shown = (selected.length ? selected : fallbacks).slice(0, 5);

    fileLines.replaceChildren();
    ingredients.replaceChildren();
    shown.forEach((ingredient, index) => {
      const color = displayColor(ingredient);
      const line = document.createElement('span');
      line.className = 'landing-ai-code-line';
      line.style.setProperty('--ingredient-color', color);
      const code = document.createElement('code');
      code.textContent = declarationFor(ingredient);
      const destination = document.createElement('i');
      line.append(code, destination);
      fileLines.appendChild(line);

      const token = document.createElement('span');
      token.className = 'landing-ai-ingredient';
      token.style.setProperty('--ingredient-color', color);
      token.style.setProperty('--ingredient-index', String(index));
      token.style.setProperty('--from-x', paths[index][0]);
      token.style.setProperty('--from-y', paths[index][1]);
      token.style.setProperty('--target-y', `${-2.2 + index * 1.15}rem`);
      token.title = ingredient.label;
      token.appendChild(createIcon(ingredient));
      ingredients.appendChild(token);
    });
  }

  function close() {
    clearTimers();
    loader.classList.remove('is-visible', 'is-complete', 'is-leaving');
    inspector.classList.remove('is-ai-loading');
    loader.hidden = true;
  }

  function dismiss() {
    return new Promise<void>((resolve) => {
      clearTimers();
      loader.classList.add('is-leaving');
      closeTimer = setTimeout(() => {
        close();
        resolve();
      }, 180);
    });
  }

  return {
    open(selectedIds: string[]) {
      clearTimers();
      render(selectedIds);
      stage.textContent = stages[0];
      loader.classList.remove('is-leaving', 'is-complete');
      loader.hidden = false;
      inspector.classList.add('is-ai-loading');
      requestAnimationFrame(() => loader.classList.add('is-visible'));
      let index = 0;
      stageTimer = setInterval(() => {
        index = (index + 1) % stages.length;
        stage.animate(
          [{ opacity: 0, transform: 'translateY(0.25rem)' }, { opacity: 1, transform: 'translateY(0)' }],
          { duration: 260, easing: 'ease-out' },
        );
        stage.textContent = stages[index];
      }, 1700);
    },
    setStage(message: string) {
      if (stageTimer) clearInterval(stageTimer);
      stageTimer = null;
      stage.textContent = message;
    },
    finish() {
      if (stageTimer) clearInterval(stageTimer);
      stageTimer = null;
      stage.textContent = 'Your composition is ready';
      loader.classList.add('is-complete');
      return new Promise<void>((resolve) => {
        closeTimer = setTimeout(() => {
          close();
          resolve();
        }, 620);
      });
    },
    dismiss,
    close,
  };
}
