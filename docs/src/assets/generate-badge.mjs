import { makeBadge } from 'badge-maker';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Devenv colors from tailwind.config.js
const devenvBlue = '#425C82';

// Extract just the icon paths (the block pattern, not the text)
// Dark boxes: white border with blue fill, light boxes: solid white
// viewBox cropped to icon bounds
const iconSvg = `<svg width="32" height="32" viewBox="66 31 349 259" fill="none" xmlns="http://www.w3.org/2000/svg">
<path d="M245.308 31V110.692H325V31L245.308 31Z" fill="${devenvBlue}" stroke="#FBFBFB" stroke-width="14"/>
<path d="M334.962 120.654V200.346H414.654V120.654H334.962Z" fill="${devenvBlue}" stroke="#FBFBFB" stroke-width="14"/>
<path d="M245.308 120.654V200.346H325V120.654H245.308Z" fill="${devenvBlue}" stroke="#FBFBFB" stroke-width="14"/>
<path d="M334.962 210.308V290H414.654V210.308H334.962Z" fill="${devenvBlue}" stroke="#FBFBFB" stroke-width="14"/>
<path d="M245.308 210.308V290H325V210.308H245.308Z" fill="#FBFBFB"/>
<path d="M155.654 210.308V290H235.346V210.308H155.654Z" fill="#FBFBFB"/>
<path d="M66 210.308V290H145.692V210.308H66Z" fill="#FBFBFB"/>
<path d="M155.654 120.654V200.346H235.346V120.654H155.654Z" fill="#FBFBFB"/>
</svg>`;

const logoBase64 = `data:image/svg+xml;base64,${Buffer.from(iconSvg).toString('base64')}`;

const badge = makeBadge({
  label: '',
  message: 'built with devenv',
  color: devenvBlue,
  style: 'flat',
  logoBase64: logoBase64,
});

// Post-process to make "devenv" bold
const boldBadge = badge
  .replace(/>built with devenv</g, '>built with <tspan font-weight="bold">devenv</tspan><');

const outputPath = path.join(__dirname, 'devenv-badge.svg');
fs.writeFileSync(outputPath, boldBadge);
console.log(`Badge generated: ${outputPath}`);
