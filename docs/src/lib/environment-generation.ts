export interface GeneratedEnvironment {
  nix: string;
  yaml: string;
}

const endpoint = import.meta.env?.DEV ? '/__devenv_api/api/generate' : '/api/generate';

export function hasMeaningfulYaml(text: string) {
  return text.split(/\r?\n/).some((line) => {
    const content = line.trim();
    return Boolean(content && !content.startsWith('#') && content !== '---' && content !== '...');
  });
}

export async function generateEnvironment(prompt: string, signal: AbortSignal): Promise<GeneratedEnvironment> {
  const query = new URLSearchParams({ q: prompt });
  const response = await fetch(`${endpoint}?${query}`, {
    method: 'POST',
    headers: { Accept: 'application/json' },
    signal,
  });
  const payload = await response.json().catch(() => null);

  if (!response.ok) {
    throw new Error(typeof payload?.error === 'string' ? payload.error : 'AI generation failed');
  }
  if (typeof payload?.devenv_nix !== 'string' || !payload.devenv_nix.trim()) {
    throw new Error('AI returned an empty devenv.nix');
  }

  return {
    nix: payload.devenv_nix.replace(/\r\n?/g, '\n'),
    yaml: typeof payload.devenv_yaml === 'string' ? payload.devenv_yaml.replace(/\r\n?/g, '\n') : '',
  };
}
