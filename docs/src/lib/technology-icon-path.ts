const iconPattern = /^[a-z0-9-]+$/;
const colorPattern = /^#[0-9a-f]{6}$/i;

export function technologyIconFilename(icon: string, color: string) {
  if (!iconPattern.test(icon)) throw new Error(`Invalid technology icon slug: ${icon}`);
  if (!colorPattern.test(color)) throw new Error(`Invalid technology icon color: ${color}`);
  return `${icon}-${color.slice(1).toLowerCase()}.svg`;
}

export function technologyIconPath(icon: string, color: string) {
  return `/technology-icons/${technologyIconFilename(icon, color)}`;
}
