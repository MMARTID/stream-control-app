import { api, type Settings } from "./api";
import { state } from "./state";
import { run, toast } from "./util";

/** Guarda un cambio de ajustes al momento (interruptores y botones), sin pasar por «Guardar cambios». */
export async function updateSettings(mutate: (s: Settings) => void, okMessage?: string): Promise<boolean> {
  if (!state.snap) return false;
  const next = structuredClone(state.snap.settings);
  mutate(next);
  const done = await run(api.saveSettings(next).then(() => true));
  if (done && okMessage) toast(okMessage, "ok");
  return !!done;
}
