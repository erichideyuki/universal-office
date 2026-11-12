// F7 sem o motor da revisão: tenta subir o motor de novo uma vez e avisa que a
// revisão está indisponível — antes dizia "A verificação ortográfica foi
// concluída", como se o texto não tivesse erros.
import { describe, expect, it, vi } from 'vitest';
import { spellingDialog, type DialogEnv } from './chrome-dialogs.js';

type State = 'off' | 'idle' | 'starting' | 'ready' | 'unavailable';

function env(state: State, after: State = state): { env: DialogEnv; flash: ReturnType<typeof vi.fn>; retry: ReturnType<typeof vi.fn> } {
  const flash = vi.fn();
  const retry = vi.fn();
  let current = state;
  const ed = {
    get spellingState() { return current; },
    retrySpelling: () => { retry(); current = after; },
    nextMisspelling: async () => null,
  };
  return { env: { ed, app: { ui: { flash } }, lang: 'pt' } as unknown as DialogEnv, flash, retry };
}

describe('F7 (Ortografia) e o estado do motor', () => {
  it('motor indisponível: tenta de novo uma vez e avisa que a revisão está indisponível', async () => {
    const t = env('unavailable');
    await spellingDialog(t.env);
    expect(t.retry).toHaveBeenCalledTimes(1);
    expect(t.flash).toHaveBeenCalledWith('A revisão ortográfica está indisponível.');
  });

  it('a nova tentativa deu certo e não há erros: revisão concluída', async () => {
    const t = env('unavailable', 'ready');
    await spellingDialog(t.env);
    expect(t.flash).toHaveBeenCalledWith('A verificação ortográfica foi concluída.');
  });

  it('sem motor registrado: indisponível, sem tentar de novo', async () => {
    const t = env('off');
    await spellingDialog(t.env);
    expect(t.retry).not.toHaveBeenCalled();
    expect(t.flash).toHaveBeenCalledWith('A revisão ortográfica está indisponível.');
  });

  it('motor pronto e sem erros: revisão concluída', async () => {
    const t = env('ready');
    await spellingDialog(t.env);
    expect(t.retry).not.toHaveBeenCalled();
    expect(t.flash).toHaveBeenCalledWith('A verificação ortográfica foi concluída.');
  });
});
