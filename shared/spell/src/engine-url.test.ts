// Motor da revisão por arquivos do próprio domínio (hospedeiro com CSP sem
// blob:/data: em script-src): o Worker sobe pela URL do pacote, o glue do
// emscripten vem por import() de URL e uma falha do motor fica memorizada —
// sem isso, cada pintura tentava subir o motor de novo (milhares de erros de CSP).
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SpellChecker, type SpellCheckerInternalOptions, type Transport } from './checker.js';
import { spellEngineMain, type EngineResponse } from './engine.js';
import { glue, wasmBase64 } from './hunspell/wasm.js';
import { registerDictionary, registerSpellcheck, type SpellcheckModule } from './sources.js';
import { memoryStore } from './user-dictionary.js';

const dir = mkdtempSync(join(tmpdir(), 'uo-spell-'));
const glueFile = join(dir, 'hunspell.js');
writeFileSync(glueFile, glue);
const glueUrl = pathToFileURL(glueFile).href;
const missingUrl = pathToFileURL(join(dir, 'nao-existe.js')).href;

const make = (opts: SpellCheckerInternalOptions = {}) => new SpellChecker({ store: memoryStore(), ...opts });

/** Worker que nunca sobe (como um script bloqueado): o motor tenta o thread. */
const failingWorker = (): Transport => {
  const t: Transport = {
    kind: 'worker',
    post: () => setTimeout(() => t.onError(new Error('worker bloqueado')), 1),
    onResponse: () => {},
    onError: () => {},
    dispose: () => {},
  };
  return t;
};

const initOn = async (url: string | undefined): Promise<EngineResponse> => {
  const replies: EngineResponse[] = [];
  const scope = { onmessage: null as null | ((ev: { data: unknown }) => Promise<void>), postMessage: (m: EngineResponse) => replies.push(m) };
  spellEngineMain(scope as never, url);
  await scope.onmessage!({ data: { id: 1, op: 'init', glue: '', wasmBase64 } });
  return replies[0]!;
};

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  registerSpellcheck(null);
});

describe('motor por URL', () => {
  it('init com a URL do glue importa o módulo pela URL, sem blob: nem data:', async () => {
    const blob = vi.spyOn(URL, 'createObjectURL');
    expect((await initOn(glueUrl)).error).toBeUndefined();
    expect(blob).not.toHaveBeenCalled();
  }, 60_000);

  it('o wasm só é decodificado depois que o glue carrega', async () => {
    const atob = vi.spyOn(globalThis, 'atob');
    const reply = await initOn(missingUrl);
    expect(reply.error).toBeTruthy();
    expect(atob).not.toHaveBeenCalled();
  });

  it('com workerUrl o Worker sobe pela URL do pacote, como módulo, sem blob:', async () => {
    const created: { url: string; opts: WorkerOptions }[] = [];
    class FakeWorker {
      onmessage: ((ev: MessageEvent) => void) | null = null;
      onerror: ((ev: ErrorEvent) => void) | null = null;
      constructor(url: string | URL, opts: WorkerOptions) { created.push({ url: String(url), opts }); }
      postMessage(): void { setTimeout(() => this.onerror?.({ message: 'bloqueado', preventDefault: () => {} } as ErrorEvent), 1); }
      terminate(): void {}
    }
    vi.stubGlobal('Worker', FakeWorker);
    const blob = vi.spyOn(URL, 'createObjectURL');
    const module: SpellcheckModule = { glue, wasmBase64, workerUrl: 'https://app.example/vendor/spellcheck/worker.js', glueUrl };
    registerSpellcheck(async () => module);
    registerDictionary('pt-BR', () => import('./dicts/pt-BR.js'));
    const sc = make();
    expect(await sc.check('computador', 'pt-BR')).toBe(true);
    expect(created).toEqual([{ url: 'https://app.example/vendor/spellcheck/worker.js', opts: { type: 'module', name: 'universal-office-spell' } }]);
    expect(blob).not.toHaveBeenCalled();
    expect(sc.mode).toBe('thread');
    sc.dispose();
  }, 60_000);
});

describe('falha do motor memorizada', () => {
  it('Worker e thread falhando: as revisões seguintes não sobem o motor de novo', async () => {
    const loader = vi.fn(async (): Promise<SpellcheckModule> => ({ glue, wasmBase64, workerUrl: 'https://x/worker.js', glueUrl: missingUrl }));
    registerSpellcheck(loader);
    registerDictionary('pt-BR', () => import('./dicts/pt-BR.js'));
    const sc = make({ workerFactory: failingWorker });
    expect(await sc.check('palavrra', 'pt-BR')).toBe(true);
    expect(await sc.check('errrada', 'pt-BR')).toBe(true);
    expect(loader).toHaveBeenCalledTimes(1);
    expect(sc.dictionaries()).toEqual([]);
    expect(sc.engineState).toBe('unavailable');
    sc.dispose();
  }, 60_000);

  it('retry() dá uma nova chance ao motor', async () => {
    let url = missingUrl;
    const loader = vi.fn(async (): Promise<SpellcheckModule> => ({ glue, wasmBase64, glueUrl: url }));
    registerSpellcheck(loader);
    registerDictionary('pt-BR', () => import('./dicts/pt-BR.js'));
    const sc = make({ worker: false });
    expect(await sc.check('computador', 'pt-BR')).toBe(true);
    expect(sc.engineState).toBe('unavailable');
    url = glueUrl;
    sc.retry();
    expect(sc.engineState).toBe('idle');
    expect(await sc.check('comptador', 'pt-BR')).toBe(false);
    expect(sc.engineState).toBe('ready');
    expect(loader).toHaveBeenCalledTimes(2);
    sc.dispose();
  }, 60_000);
});
